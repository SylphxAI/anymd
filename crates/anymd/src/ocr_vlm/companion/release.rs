//! Release asset names and the version-bound signed manifest.
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// The release-asset platform key (the npm package suffix) of this build.
pub fn companion_platform() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("darwin-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("darwin-x64")
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        Some("linux-x64-gnu")
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "aarch64",
        target_env = "gnu"
    )) {
        Some("linux-arm64-gnu")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("win32-x64-msvc")
    } else {
        None
    }
}

/// Release asset name of the companion for `platform`.
pub fn companion_asset(platform: &str) -> String {
    let suffix = if platform.starts_with("win32") {
        ".exe"
    } else {
        ""
    };
    format!("anymd-ocr-vlm-{platform}{suffix}")
}

pub const COMPANION_MANIFEST: &str = "anymd-ocr-vlm-SHA256SUMS";

/// The SHA-256 a `sha256sum`-style manifest lists for `asset`.
pub fn manifest_hash(manifest: &str, asset: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == asset && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

pub(super) const COMPANION_SIGNATURE: &str = "anymd-ocr-vlm-SHA256SUMS.sig";

/// The bytes the release signs: a line naming the version, then the manifest.
/// A signature for another version's manifest therefore fails here.
pub(super) fn signed_message(version: &str, manifest: &str) -> Vec<u8> {
    format!("anymd-ocr-vlm {version}\n{manifest}").into_bytes()
}

pub(super) fn verify_manifest(
    version: &str,
    manifest: &str,
    signature_b64: &str,
    keys: &[&str],
) -> Result<(), String> {
    let signature = URL_SAFE_NO_PAD
        .decode(signature_b64.trim())
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or("OCR engine manifest signature is malformed")?;
    let message = signed_message(version, manifest);
    let verified = keys.iter().any(|key| {
        URL_SAFE_NO_PAD
            .decode(key)
            .ok()
            .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
            .and_then(|raw| VerifyingKey::from_bytes(&raw).ok())
            .is_some_and(|key| key.verify(&message, &signature).is_ok())
    });
    if verified {
        Ok(())
    } else {
        Err("OCR engine manifest signature is not valid for this version".into())
    }
}
