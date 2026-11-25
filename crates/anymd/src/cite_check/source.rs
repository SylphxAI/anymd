use std::io::Read;
use std::path::{Path, PathBuf};

use crate::schema::PdfEvidenceSource;

pub(super) const MAX_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn snapshot(
    source: &PdfEvidenceSource,
    request: &Path,
) -> Result<(PathBuf, String), String> {
    // Snapshot once: the hash, native extraction and OCR consume identical bytes,
    // even when a caller edits its local file while this request is in flight.
    // URL bytes come from the existing SSRF-safe owner without an independent
    // temporary file that could survive worker termination.
    let bytes = if let Some(path) = &source.path {
        if !std::fs::metadata(path)
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Err("PDF source must be a regular file.".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        bytes
    } else {
        anymd_core::url_fetch::fetch_url(source.url.as_deref().ok_or("Missing PDF source.")?)?.bytes
    };
    if bytes.len() as u64 > MAX_BYTES {
        return Err("PDF exceeds 256 MiB.".into());
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err("cite_check accepts PDF sources only.".into());
    }
    let snapshot = request.with_file_name("source.pdf");
    std::fs::write(&snapshot, bytes).map_err(|e| e.to_string())?;
    let hash = anymd_core::hash_file(&snapshot, MAX_BYTES)
        .map_err(|e| e.message)?
        .source_hash;
    Ok((snapshot, hash))
}
