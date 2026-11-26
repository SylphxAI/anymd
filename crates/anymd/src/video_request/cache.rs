use crate::source_access::SourceAccessPolicy;
use crate::video_evidence::VideoEvidence;
use anymd_formats::video::timeline;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Instant;

pub(super) fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(super) fn remaining(deadline: Instant) -> Result<u64, String> {
    let ms = deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64;
    if ms == 0 {
        Err("video request deadline exceeded".into())
    } else {
        Ok(ms)
    }
}
/// Snapshot the admitted representation so hashes and extraction cannot diverge.
pub(super) fn materialize(
    source: &str,
    policy: &SourceAccessPolicy,
    deadline: Instant,
) -> Result<tempfile::NamedTempFile, String> {
    remaining(deadline)?;
    let bytes = if crate::document::is_url(source) {
        anymd_core::url_fetch::fetch_url_deadline(source, deadline)?.bytes
    } else {
        let admitted = policy.admit_path(source)?;
        let mut file = std::fs::File::open(&admitted).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(256 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        bytes
    };
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("video representation exceeds 256 MiB".into());
    }
    remaining(deadline)?;
    let mut file = tempfile::Builder::new()
        .suffix(".media")
        .tempfile()
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(file)
}

fn artifact(key: &str) -> Result<PathBuf, String> {
    anymd_formats::cache::cache_dir()
        .map(|p| p.join("images").join(format!("video-{key}.json")))
        .ok_or("video cache unavailable".into())
}
pub(super) fn cache_put<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    let path = artifact(key)?;
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub(super) fn cache_get<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    let path = artifact(key).ok()?;
    let bytes = std::fs::read(&path).ok()?;
    let result = serde_json::from_slice(&bytes).ok()?;
    anymd_formats::cache::touch(&path);
    Some(result)
}

/// Only a fully complete manifest is cached; the key has no deadline, so a
/// deadline-limited partial result must never be reused.
pub(super) fn cacheable(evidence: &VideoEvidence) -> bool {
    let components = &evidence.timeline.components;
    [
        &components.scenes,
        &components.chapters,
        &components.subtitles,
        &components.caption,
        &components.ocr,
        &components.transcript,
    ]
    .iter()
    .all(|c| {
        matches!(
            c.status,
            timeline::Status::Ok | timeline::Status::NotRequested
        )
    })
}
