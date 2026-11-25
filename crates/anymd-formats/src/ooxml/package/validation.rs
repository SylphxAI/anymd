use std::io::{Cursor, Read};

use crate::ConvertError;

use super::{invalid, MAX_ENTRIES, MAX_ENTRY_BYTES};

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
