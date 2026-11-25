use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

use crate::limits::Limits;
use crate::ConvertError;

use super::super::{parse_xml, Element};
use super::{invalid, validate_archive, MAX_ENTRY_BYTES};

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
}

/// An immutable, independently measured inflation charge for one entry.
#[derive(Debug)]
pub(crate) struct EntryInflation {
    pub(crate) index: usize,
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) inflated: usize,
}
