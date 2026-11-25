//! Download, verify, and install the companion and its verified stamps.
use super::release::{
    companion_asset, companion_platform, manifest_hash, verify_manifest, COMPANION_MANIFEST,
    COMPANION_SIGNATURE,
};
use super::{companion_file_name, COMPANION_HASH_FILE, COMPANION_VERSION_FILE};
use crate::ocr_vlm::{model_cache::verified, COMPANION_PUBLIC_KEYS};
use std::io::{Read, Write};
use std::path::Path;

const COMPANION_MAX_BYTES: u64 = 256 * 1024 * 1024;

pub(in crate::ocr_vlm) fn install_companion(root: &Path) -> Result<(), String> {
    let platform = companion_platform().ok_or(
        "No VLM OCR engine is published for this platform; tesseract OCR remains available",
    )?;
    let version = env!("CARGO_PKG_VERSION");
    let base = format!("{}/v{version}", crate::RELEASE_DOWNLOAD_BASE);
    install_companion_from(root, &base, platform, version, COMPANION_PUBLIC_KEYS, true)
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
pub(super) fn install_companion_from(
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
