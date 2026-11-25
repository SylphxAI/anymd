//! Select and validate the companion before launching an OCR worker.
use super::install::install_companion;
use super::{companion_file_name, COMPANION_HASH_FILE, COMPANION_VERSION_FILE};
use crate::ocr_vlm::{installed_at, model_cache::file_digest, root, SETUP_NOTICE};
use std::path::{Path, PathBuf};

pub(super) const INTEGRITY_NOTICE: &str =
    "the OCR engine failed its integrity check; run anymd setup ocr";

/// A build that links the engine (`--features ocr-vlm`, which includes the
/// companion itself) runs it in-process and needs no companion file.
pub(crate) fn companion_installed_at(root: &Path) -> bool {
    cfg!(feature = "ocr-vlm")
        || (root.join(companion_file_name()).is_file()
            && std::fs::read_to_string(root.join(COMPANION_VERSION_FILE))
                .ok()
                .as_deref()
                == Some(env!("CARGO_PKG_VERSION")))
}

/// Weights and engine are both in place.
pub(crate) fn ready_at(root: &Path) -> bool {
    installed_at(root) && companion_installed_at(root)
}

/// The executable that runs the `__ocr-vlm-worker` protocol: this binary when
/// it links the engine, otherwise the installed companion.
pub(crate) fn worker_command() -> Result<PathBuf, String> {
    if cfg!(feature = "ocr-vlm") {
        return std::env::current_exe().map_err(|e| e.to_string());
    }
    worker_command_at(&root()?)
}

fn worker_command_at(root: &Path) -> Result<PathBuf, String> {
    worker_command_with(root, &refreshed_companion)
}

pub(super) fn worker_command_with(
    root: &Path,
    refresh: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    refresh_if_stale(root, refresh)?;
    if !ready_at(root) {
        return Err(SETUP_NOTICE.into());
    }
    if !companion_intact_at(root)? {
        // A companion damaged on disk after install: treat it like a stale
        // one when the weights are installed, otherwise refuse. Never run it.
        if installed_at(root) {
            refresh(root).map_err(|e| format!("{INTEGRITY_NOTICE} ({e})"))?;
        }
        if !companion_intact_at(root)? {
            return Err(INTEGRITY_NOTICE.into());
        }
    }
    Ok(root.join(companion_file_name()))
}

/// The companion on disk still matches the hash stored at install. A build
/// that links the engine has no companion file to damage.
pub(in crate::ocr_vlm) fn companion_intact_at(root: &Path) -> Result<bool, String> {
    if cfg!(feature = "ocr-vlm") {
        return Ok(true);
    }
    let Ok(stored) = std::fs::read_to_string(root.join(COMPANION_HASH_FILE)) else {
        return Ok(false);
    };
    // Hashed on every launch: ~10 MB takes milliseconds against seconds of
    // inference, and a cache keyed by size and mtime could be fooled.
    let Some(actual) = file_digest(&root.join(companion_file_name()))? else {
        return Ok(false);
    };
    Ok(stored.trim() == actual)
}

/// Weights are installed (the user opted in) but the engine is missing or from
/// another version: refresh the small engine, never the weights. Setup consent
/// covers this.
pub(in crate::ocr_vlm) fn refresh_if_stale(
    root: &Path,
    refresh: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    if !installed_at(root) || companion_installed_at(root) {
        return Ok(());
    }
    refresh(root).map_err(|e| {
        format!(
            "the OCR engine for v{} could not be refreshed: {e}; run anymd setup ocr",
            env!("CARGO_PKG_VERSION")
        )
    })
}

/// One refresh attempt per process, so a failing network is not retried on
/// every page.
pub(in crate::ocr_vlm) fn refreshed_companion(root: &Path) -> Result<(), String> {
    static REFRESH: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    REFRESH.get_or_init(|| install_companion(root)).clone()
}
