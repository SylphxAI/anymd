//! Install and select the signed companion executable.

mod install;
mod release;
mod worker;

pub(super) use install::install_companion;
pub use release::{companion_asset, companion_platform, manifest_hash, COMPANION_MANIFEST};
pub(crate) use worker::{companion_installed_at, ready_at, worker_command};
pub(super) use worker::{companion_intact_at, refresh_if_stale, refreshed_companion};

const COMPANION_VERSION_FILE: &str = "anymd-ocr-vlm.version";
/// The SHA-256 of the companion as verified at install (against the signed
/// manifest), checked again before every launch.
const COMPANION_HASH_FILE: &str = "anymd-ocr-vlm.sha256";

/// `anymd-ocr-vlm`, the separate executable that holds the in-process VLM
/// engine. `anymd setup ocr` installs it next to the weights.
pub fn companion_file_name() -> &'static str {
    if cfg!(windows) {
        "anymd-ocr-vlm.exe"
    } else {
        "anymd-ocr-vlm"
    }
}

#[cfg(all(test, any(unix, not(feature = "ocr-vlm"))))]
use std::path::Path;
#[cfg(all(test, unix))]
use {
    super::model_cache::file_digest,
    install::install_companion_from,
    std::io::{Read, Write},
};
#[cfg(test)]
use {
    super::COMPANION_PUBLIC_KEYS,
    base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine},
    ed25519_dalek::VerifyingKey,
    release::{signed_message, verify_manifest},
};
#[cfg(all(test, not(feature = "ocr-vlm")))]
use {
    super::{installed_at, SETUP_NOTICE},
    worker::{worker_command_with, INTEGRITY_NOTICE},
};

#[cfg(test)]
mod tests;
