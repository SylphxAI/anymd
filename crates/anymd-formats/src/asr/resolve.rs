use super::download::download_model;
use super::{ModelSpec, CACHE_ENV, MODEL_ENV};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Only the pinned model is accepted, including files supplied by the caller.
pub fn ensure_model(spec: &ModelSpec, env: &str, download: bool) -> Result<PathBuf, String> {
    let supplied = std::env::var_os(env)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    resolve_model(spec, supplied.as_deref(), models_dir().as_deref(), download)
}

pub(super) fn resolve_model(
    spec: &ModelSpec,
    supplied: Option<&Path>,
    cache: Option<&Path>,
    download: bool,
) -> Result<PathBuf, String> {
    if let Some(path) = supplied {
        verify_model(spec, path)?;
        return Ok(path.to_path_buf());
    }
    let dir = cache.ok_or_else(|| format!("no model cache; set {CACHE_ENV} or {MODEL_ENV}"))?;
    let path = dir.join(spec.file_name());
    if path.exists() {
        verify_model(spec, &path)?;
        Ok(path)
    } else if download {
        download_model(spec, dir, spec.base_url)
    } else {
        Err(format!(
            "pinned model {} is not installed; use download_asr_model: true / --download-asr-model to allow a download, or install the pinned model locally",
            spec.name
        ))
    }
}

#[cfg(feature = "native")]
pub(super) fn verify_model(spec: &ModelSpec, path: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("model {}: {e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() != spec.bytes {
        return Err(format!(
            "model {} has the wrong size; expected the pinned {} ({} bytes)",
            path.display(),
            spec.name,
            spec.bytes
        ));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let digest = hex::encode(hash.finalize());
    if digest != spec.sha256 {
        return Err(format!(
            "model checksum mismatch: {} is not the pinned {}",
            path.display(),
            spec.name
        ));
    }
    Ok(())
}

#[cfg(not(feature = "native"))]
pub(super) fn verify_model(_spec: &ModelSpec, _path: &Path) -> Result<(), String> {
    Err("transcripts need the native anymd build".into())
}

/// `ANYMD_CACHE_DIR/models`, else the platform cache directory + `anymd/models`.
pub fn models_dir() -> Option<PathBuf> {
    models_dir_from(&|key: &str| std::env::var_os(key))
}

pub(super) fn models_dir_from(get: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |key: &str| get(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(root) = var(CACHE_ENV) {
        return Some(root.join("models"));
    }
    let base = if cfg!(windows) {
        var("LOCALAPPDATA").map(|p| p.join("anymd").join("cache"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|p| p.join("Library").join("Caches").join("anymd"))
    } else {
        var("XDG_CACHE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|p| p.join(".cache")))
            .map(|p| p.join("anymd"))
    };
    base.map(|p| p.join("models"))
}
