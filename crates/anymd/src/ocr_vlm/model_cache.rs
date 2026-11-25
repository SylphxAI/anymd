//! Manage the opted-in model cache and its pinned weight files.
use super::weights;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(crate) fn model_revision() -> &'static str {
    weights::REVISION
}

pub fn root() -> Result<PathBuf, String> {
    anymd_formats::cache::cache_dir()
        .map(|p| p.join("models/docvlm-v1"))
        .ok_or_else(|| "Cannot locate anymd cache; set ANYMD_CACHE_DIR".into())
}

pub(crate) fn installed_at(root: &Path) -> bool {
    weights::FILES.iter().all(|f| root.join(f.path).is_file())
        && std::fs::read_to_string(root.join("installed"))
            .ok()
            .as_deref()
            == Some(weights::REVISION)
}

pub(super) fn install_weights(root: &Path) -> Result<(), String> {
    for entry in weights::FILES {
        let path = root.join(entry.path);
        if verified(&path, entry.sha256)? {
            continue;
        }
        let parent = path.parent().ok_or("Invalid model path")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        eprintln!("Downloading OCR model file {}", entry.path);
        let mut response = ureq::get(entry.url).call().map_err(|e| e.to_string())?;
        let mut reader = response.body_mut().as_reader().take(entry.bytes + 1);
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        let n = std::io::copy(&mut reader, &mut temporary).map_err(|e| e.to_string())?;
        if n != entry.bytes || !verified(temporary.path(), entry.sha256)? {
            return Err(format!("OCR model hash/size mismatch: {}", entry.path));
        }
        temporary.flush().map_err(|e| e.to_string())?;
        temporary.persist(&path).map_err(|e| e.to_string())?;
    }
    // This is the explicit CPU opt-in. Only write it after every hash passes.
    std::fs::write(root.join("installed"), weights::REVISION).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn verified(path: &Path, hash: &str) -> Result<bool, String> {
    Ok(file_digest(path)?.is_some_and(|actual| actual == hash))
}

pub(super) fn file_digest(path: &Path) -> Result<Option<String>, String> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Ok(None);
    };
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(Some(hex::encode(digest.finalize())))
}

#[cfg(test)]
mod tests;
