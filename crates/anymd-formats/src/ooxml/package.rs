use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

use crate::limits::Limits;
use crate::ConvertError;

use super::{parse_xml, Element};

/// Largest uncompressed zip entry we will inflate.
pub(crate) const MAX_ENTRY_BYTES: u64 = 200 * 1024 * 1024;
/// Largest number of entries a container may declare.
pub(crate) const MAX_ENTRIES: usize = 10_000;

pub(crate) fn invalid(message: impl std::fmt::Display) -> ConvertError {
    ConvertError::Invalid(message.to_string())
}

/// A zip container with entry-count and inflated-size limits enforced.
pub(crate) struct Package<'a> {
    pub(crate) limits: Limits,
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
    names: HashSet<String>,
    folded_names: HashMap<String, String>,
}

impl<'a> Package<'a> {
    pub(crate) fn open(bytes: &'a [u8], kind: &str) -> Result<Self, ConvertError> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| invalid(format!("not a valid {kind} file: {e}")))?;
        validate_archive(bytes, &mut archive)?;
        let mut names = HashSet::with_capacity(archive.len());
        let mut folded_names = HashMap::with_capacity(archive.len());
        for name in archive.file_names().filter_map(|name| name.ok()) {
            // Keep the same first fallback when an archive has case variants.
            folded_names
                .entry(name.to_ascii_lowercase())
                .or_insert_with(|| name.to_string());
            names.insert(name.to_string());
        }
        Ok(Self {
            limits: Limits::default(),
            archive,
            names,
            folded_names,
        })
    }

    /// Measure each entry independently. An entry's index is its accounting
    /// identity; payload offsets are only used to locate later parser reads.
    pub(crate) fn preflight(&mut self) -> Result<Vec<EntryInflation>, ConvertError> {
        let mut entries = Vec::with_capacity(self.archive.len());
        for index in 0..self.archive.len() {
            let file = self
                .archive
                .by_index(index)
                .map_err(|e| invalid(format!("cannot read spreadsheet entry: {e}")))?;
            let start = file
                .data_start()
                .ok_or_else(|| invalid("spreadsheet entry has no payload offset"))?;
            let end = start
                .checked_add(file.compressed_size())
                .ok_or_else(|| invalid("spreadsheet payload offset overflow"))?;
            let bytes = self.limits.read(file.take(MAX_ENTRY_BYTES + 1))?;
            if bytes.len() as u64 > MAX_ENTRY_BYTES {
                return Err(invalid(
                    "spreadsheet entry exceeds its uncompressed size limit",
                ));
            }
            entries.push(EntryInflation {
                index,
                start,
                end,
                inflated: bytes.len(),
            });
        }
        entries.sort_unstable_by_key(|entry| entry.start);
        Ok(entries)
    }

    fn resolve_name(&self, name: &str) -> Option<String> {
        let name = name.trim_start_matches('/');
        // Use decoded names for exact precedence, including CP437 ZIP entries.
        // Missing optional parts use the folded index too, without scanning
        // every archive entry.
        if self.names.contains(name) {
            return Some(name.to_string());
        }
        self.folded_names.get(&name.to_ascii_lowercase()).cloned()
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.resolve_name(name).is_some()
    }

    /// Read one entry, or `None` when it does not exist.
    pub(crate) fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, ConvertError> {
        self.limits.check()?;
        let Some(name) = self.resolve_name(name) else {
            return Ok(None);
        };
        let file = self
            .archive
            .by_name(&name)
            .map_err(|e| invalid(format!("cannot read {name}: {e}")))?;
        if file.size() > MAX_ENTRY_BYTES {
            return Err(invalid(format!(
                "{name} is larger than {MAX_ENTRY_BYTES} bytes uncompressed"
            )));
        }
        // Count actual inflated bytes, including repeated optional reads.
        let out = self.limits.read(file.take(MAX_ENTRY_BYTES + 1))?;
        if out.len() as u64 > MAX_ENTRY_BYTES {
            return Err(invalid(format!(
                "{name} is larger than {MAX_ENTRY_BYTES} bytes uncompressed"
            )));
        }
        Ok(Some(out))
    }

    /// Read and parse an XML entry, or `None` when it does not exist.
    pub(crate) fn xml(&mut self, name: &str) -> Result<Option<Element>, ConvertError> {
        match self.read(name)? {
            Some(bytes) => parse_xml(&bytes)
                .map(Some)
                .map_err(|e| invalid(format!("{name}: {e}"))),
            None => Ok(None),
        }
    }

    /// Relationships of a part (`dir/_rels/file.rels`), keyed by id.
    pub(crate) fn rels(&mut self, part: &str) -> Result<Rels, ConvertError> {
        let (dir, file) = split_part(part);
        let rels_name = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let mut rels = Rels::default();
        let Some(root) = self.xml(&rels_name)? else {
            return Ok(rels);
        };
        for rel in root.children_named("Relationship") {
            let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) else {
                continue;
            };
            let external = rel
                .attr("TargetMode")
                .is_some_and(|m| m.eq_ignore_ascii_case("External"));
            let target = if external {
                target.to_string()
            } else {
                resolve_target(dir, target)
            };
            rels.by_id.insert(
                id.to_string(),
                Rel {
                    kind: rel.attr("Type").unwrap_or_default().to_string(),
                    target,
                    external,
                },
            );
            rels.order.push(id.to_string());
        }
        Ok(rels)
    }

    /// The main part named by the package-level officeDocument relationship.
    pub(crate) fn main_part(&mut self, fallback: &str) -> Result<String, ConvertError> {
        let rels = self.rels("")?;
        let found = rels
            .first_of_type("/officeDocument")
            .map(|r| r.target.clone());
        Ok(found
            .filter(|t| self.has(t))
            .unwrap_or_else(|| fallback.to_string()))
    }
}

/// Reject containers that declare too many entries or oversized members.
pub(crate) fn check_limits<R: Read + std::io::Seek>(
    archive: &zip::ZipArchive<R>,
) -> Result<(), ConvertError> {
    if archive.len() > MAX_ENTRIES {
        return Err(invalid(format!(
            "container has {} entries (limit {MAX_ENTRIES})",
            archive.len()
        )));
    }
    Ok(())
}

/// An immutable, independently measured inflation charge for one entry.
#[derive(Debug)]
pub(crate) struct EntryInflation {
    pub(crate) index: usize,
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) inflated: usize,
}

/// Validate the complete container before any format-specific parser reads it.
pub(crate) fn validate_archive(
    bytes: &[u8],
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<(), ConvertError> {
    check_limits(archive)?;
    // Duplicate names hide directory entries in the reader's name index.
    if archive.has_duplicate_names() {
        return Err(invalid("archive has duplicate entry names"));
    }
    if archive
        .has_overlapping_entries()
        .map_err(|e| invalid(format!("invalid archive entry: {e}")))?
    {
        return Err(invalid("archive entries overlap"));
    }
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|e| invalid(format!("bad zip entry: {e}")))?;
        // Compare the original header fields, without imposing equality on
        // unrelated fields such as modification timestamps.
        let local = usize::try_from(file.header_start())
            .ok()
            .and_then(|start| bytes.get(start..))
            .filter(|header| header.len() >= 30)
            .ok_or_else(|| invalid("invalid archive entry header"))?;
        let central = usize::try_from(file.central_header_start())
            .ok()
            .and_then(|start| bytes.get(start..))
            .filter(|header| header.len() >= 46)
            .ok_or_else(|| invalid("invalid archive directory entry"))?;
        if local[6..8] != central[8..10] {
            return Err(invalid("archive entry flags disagree"));
        }
        if local[8..10] != central[10..12] {
            return Err(invalid("archive entry methods disagree"));
        }
        // Effective names may have been replaced by Unicode-path extra fields.
        let local_length = usize::from(u16::from_le_bytes([local[26], local[27]]));
        let central_length = usize::from(u16::from_le_bytes([central[28], central[29]]));
        let local_name = local
            .get(30..30 + local_length)
            .ok_or_else(|| invalid("invalid archive entry name"))?;
        let central_name = central
            .get(46..46 + central_length)
            .ok_or_else(|| invalid("invalid archive directory name"))?;
        if local_name != central_name {
            return Err(invalid("archive entry names disagree"));
        }
        let descriptor = u16::from_le_bytes([local[6], local[7]]) & 8 != 0;
        let crc = u32::from_le_bytes(local[14..18].try_into().unwrap());
        // A streamed local header may leave the CRC zero until its descriptor.
        if crc != file.crc32() && !(descriptor && crc == 0) {
            return Err(invalid("archive entry checksums disagree"));
        }
        if !descriptor {
            let (compressed, inflated) = local_sizes(local, local_length)?;
            if compressed != file.compressed_size() || inflated != file.size() {
                return Err(invalid("archive entry sizes disagree"));
            }
        }
        if file.size() > MAX_ENTRY_BYTES {
            return Err(invalid(format!(
                "{} is larger than {MAX_ENTRY_BYTES} bytes uncompressed",
                file.name().unwrap_or_else(|_| "<unnamed>".into())
            )));
        }
    }
    Ok(())
}

/// Resolve sentinel sizes from the local ZIP64 extra field in specification
/// order (uncompressed, then compressed). Central sizes are resolved by zip.
fn local_sizes(local: &[u8], name_length: usize) -> Result<(u64, u64), ConvertError> {
    let mut compressed = u64::from(u32::from_le_bytes(local[18..22].try_into().unwrap()));
    let mut inflated = u64::from(u32::from_le_bytes(local[22..26].try_into().unwrap()));
    let sentinel = u64::from(u32::MAX);
    if compressed != sentinel && inflated != sentinel {
        return Ok((compressed, inflated));
    }
    let extra_length = usize::from(u16::from_le_bytes([local[28], local[29]]));
    let start = 30 + name_length;
    let mut extra = local
        .get(start..start + extra_length)
        .ok_or_else(|| invalid("invalid archive entry extra fields"))?;
    let mut sizes = None;
    while !extra.is_empty() {
        let header = extra
            .get(..4)
            .ok_or_else(|| invalid("invalid archive entry extra field"))?;
        let kind = u16::from_le_bytes([header[0], header[1]]);
        let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
        let data = extra
            .get(4..4 + length)
            .ok_or_else(|| invalid("invalid archive entry extra field length"))?;
        if kind == 1 && sizes.replace(data).is_some() {
            return Err(invalid("duplicate archive entry size fields"));
        }
        extra = &extra[4 + length..];
    }
    let mut sizes = sizes.ok_or_else(|| invalid("archive entry lacks extended sizes"))?;
    for size in [&mut inflated, &mut compressed] {
        if *size == sentinel {
            let value = sizes
                .get(..8)
                .ok_or_else(|| invalid("truncated archive entry extended size"))?;
            *size = u64::from_le_bytes(value.try_into().unwrap());
            sizes = &sizes[8..];
        }
    }
    Ok((compressed, inflated))
}

/// Validate a zip-based input without keeping it open.
pub(crate) fn guard_zip(bytes: &[u8]) -> Result<(), ConvertError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| invalid(format!("not a valid zip container: {e}")))?;
    validate_archive(bytes, &mut archive)
}

fn split_part(part: &str) -> (&str, &str) {
    let part = part.trim_start_matches('/');
    match part.rfind('/') {
        Some(i) => (&part[..i], &part[i + 1..]),
        None => ("", part),
    }
}

/// Resolve a relationship target relative to the source part's directory.
pub(super) fn resolve_target(dir: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or_default();
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[derive(Debug, Clone)]
pub(crate) struct Rel {
    pub kind: String,
    pub target: String,
    pub external: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Rels {
    by_id: HashMap<String, Rel>,
    order: Vec<String>,
}

impl Rels {
    pub(crate) fn get(&self, id: &str) -> Option<&Rel> {
        self.by_id.get(id)
    }

    pub(crate) fn first_of_type(&self, suffix: &str) -> Option<&Rel> {
        self.order
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .find(|r| r.kind.ends_with(suffix))
    }

    /// Walk relationship IDs in their original order, looking up each current value.
    pub(super) fn ordered(&self) -> impl Iterator<Item = &Rel> {
        self.order.iter().filter_map(|id| self.by_id.get(id))
    }

    /// A hyperlink target for a relationship id: external URLs only.
    pub(crate) fn link(&self, id: &str) -> Option<String> {
        self.get(id)
            .filter(|r| r.external && !r.target.is_empty())
            .map(|r| r.target.clone())
    }
}
