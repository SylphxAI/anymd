//! The anymd cache directory: `ANYMD_CACHE_DIR`, else the platform cache
//! directory + `anymd`. Whisper models (`models/`) and exported document
//! images (`images/`) live under it.

use std::ffi::OsString;
use std::path::PathBuf;

/// Overrides the cache directory.
pub const CACHE_ENV: &str = "ANYMD_CACHE_DIR";

/// The cache directory, or `None` when there is no home directory to derive one from.
pub fn cache_dir() -> Option<PathBuf> {
    cache_dir_from(&|key: &str| std::env::var_os(key))
}

pub(crate) fn cache_dir_from(get: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |key: &str| get(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(root) = var(CACHE_ENV) {
        return Some(root);
    }
    if cfg!(windows) {
        var("LOCALAPPDATA").map(|p| p.join("anymd").join("cache"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|p| p.join("Library").join("Caches").join("anymd"))
    } else {
        var("XDG_CACHE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|p| p.join(".cache")))
            .map(|p| p.join("anymd"))
    }
}
