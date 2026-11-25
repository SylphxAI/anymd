//! Install and select the signed companion executable.
use super::model_cache::{file_digest, verified};
use super::{installed_at, root, COMPANION_PUBLIC_KEYS, SETUP_NOTICE};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const COMPANION_VERSION_FILE: &str = "anymd-ocr-vlm.version";
/// The SHA-256 of the companion as verified at install (against the signed
/// manifest), checked again before every launch.
const COMPANION_HASH_FILE: &str = "anymd-ocr-vlm.sha256";
const INTEGRITY_NOTICE: &str = "the OCR engine failed its integrity check; run anymd setup ocr";

/// `anymd-ocr-vlm`, the separate executable that holds the in-process VLM
/// engine. `anymd setup ocr` installs it next to the weights.
pub fn companion_file_name() -> &'static str {
    if cfg!(windows) {
        "anymd-ocr-vlm.exe"
    } else {
        "anymd-ocr-vlm"
    }
}

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

fn worker_command_with(
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
pub(super) fn companion_intact_at(root: &Path) -> Result<bool, String> {
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
pub(super) fn refresh_if_stale(
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
pub(super) fn refreshed_companion(root: &Path) -> Result<(), String> {
    static REFRESH: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    REFRESH.get_or_init(|| install_companion(root)).clone()
}

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
const COMPANION_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// The SHA-256 a `sha256sum`-style manifest lists for `asset`.
pub fn manifest_hash(manifest: &str, asset: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == asset && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

const COMPANION_SIGNATURE: &str = "anymd-ocr-vlm-SHA256SUMS.sig";

/// The bytes the release signs: a line naming the version, then the manifest.
/// A signature for another version's manifest therefore fails here.
fn signed_message(version: &str, manifest: &str) -> Vec<u8> {
    format!("anymd-ocr-vlm {version}\n{manifest}").into_bytes()
}

fn verify_manifest(
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

fn fetch_text(agent: &ureq::Agent, url: &str, what: &str, version: &str) -> Result<String, String> {
    let mut text = String::new();
    agent
        .get(url)
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => {
                format!("the OCR engine for v{version} is not published yet; retry later")
            }
            e => format!("OCR engine {what}: {e}"),
        })?
        .body_mut()
        .as_reader()
        .take(1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

pub(super) fn install_companion(root: &Path) -> Result<(), String> {
    let platform = companion_platform().ok_or(
        "No VLM OCR engine is published for this platform; tesseract OCR remains available",
    )?;
    let version = env!("CARGO_PKG_VERSION");
    let base = format!("{}/v{version}", crate::RELEASE_DOWNLOAD_BASE);
    install_companion_from(root, &base, platform, version, COMPANION_PUBLIC_KEYS, true)
}

/// Run a just-written binary's `version`. On Linux, exec fails with ETXTBSY
/// ("text file busy") while another thread's fork still holds the write handle
/// for a moment; that clears within milliseconds, so retry that one error briefly.
fn run_new_binary(path: &Path) -> std::io::Result<std::process::Output> {
    const ETXTBSY: i32 = 26;
    let mut tries = 0;
    loop {
        match std::process::Command::new(path)
            .arg("version")
            .stdin(std::process::Stdio::null())
            .output()
        {
            Err(e) if e.raw_os_error() == Some(ETXTBSY) && tries < 50 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

/// `https_only` is false only for tests against a local server.
fn install_companion_from(
    root: &Path,
    base: &str,
    platform: &str,
    version: &str,
    keys: &[&str],
    https_only: bool,
) -> Result<(), String> {
    let asset = companion_asset(platform);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(https_only)
        .build()
        .into();
    eprintln!("Downloading the OCR engine {asset}");
    let manifest = fetch_text(
        &agent,
        &format!("{base}/{COMPANION_MANIFEST}"),
        "manifest",
        version,
    )?;
    let signature = fetch_text(
        &agent,
        &format!("{base}/{COMPANION_SIGNATURE}"),
        "signature",
        version,
    )?;
    verify_manifest(version, &manifest, &signature, keys)?;
    let hash = manifest_hash(&manifest, &asset)
        .ok_or_else(|| format!("OCR engine manifest does not list {asset}"))?;
    let mut response = agent
        .get(&format!("{base}/{asset}"))
        .call()
        .map_err(|e| format!("OCR engine download: {e}"))?;
    let mut reader = response
        .body_mut()
        .as_reader()
        .take(COMPANION_MAX_BYTES + 1);
    let mut temporary = tempfile::Builder::new()
        .suffix(if cfg!(windows) { ".exe" } else { "" })
        .tempfile_in(root)
        .map_err(|e| e.to_string())?;
    let n = std::io::copy(&mut reader, &mut temporary).map_err(|e| e.to_string())?;
    temporary.flush().map_err(|e| e.to_string())?;
    if n > COMPANION_MAX_BYTES || !verified(temporary.path(), &hash)? {
        return Err(format!("OCR engine hash mismatch: {asset}"));
    }
    // Close the write handle: an open file cannot be executed.
    let path = temporary.into_temp_path();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    let output = run_new_binary(&path).map_err(|e| format!("OCR engine does not run: {e}"))?;
    if String::from_utf8_lossy(&output.stdout).trim_end() != format!("anymd-ocr-vlm {version}") {
        return Err(format!(
            "OCR engine {asset} does not report version {version}"
        ));
    }
    path.persist(root.join(companion_file_name()))
        .map_err(|e| e.to_string())?;
    // The verified hash, written atomically after the rename, so a later
    // launch can tell a damaged companion from the one that was verified.
    let mut stamp = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
    stamp
        .write_all(hash.as_bytes())
        .map_err(|e| e.to_string())?;
    stamp.flush().map_err(|e| e.to_string())?;
    stamp
        .persist(root.join(COMPANION_HASH_FILE))
        .map_err(|e| e.to_string())?;
    // The version file is what makes the engine count as installed.
    std::fs::write(root.join(COMPANION_VERSION_FILE), version).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
