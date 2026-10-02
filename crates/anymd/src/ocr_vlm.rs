//! Explicit model setup and bounded local doc-VLM worker.
use crate::command_provider::{self, CommandInvocation};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

mod weights;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OcrEngine {
    #[default]
    Auto,
    Vlm,
    Tesseract,
}

/// Retain the existing boolean MCP contract while adding named engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum OcrSelection {
    Enabled(bool),
    Engine(OcrEngine),
}
impl OcrSelection {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Engine(OcrEngine::Auto)),
            "vlm" => Ok(Self::Engine(OcrEngine::Vlm)),
            "tesseract" => Ok(Self::Engine(OcrEngine::Tesseract)),
            _ => Err("OCR needs auto, vlm or tesseract".into()),
        }
    }
    pub fn enabled(self) -> bool {
        !matches!(self, Self::Enabled(false))
    }
    pub fn engine(self) -> OcrEngine {
        match self {
            Self::Engine(e) => e,
            _ => OcrEngine::Auto,
        }
    }
}

/// A boolean enables/disables OCR but does not choose a runtime. Named modes
/// override the environment; legacy `--ocr`/`ocr: true` inherit its runtime.
pub fn resolve_engine(
    selection: Option<OcrSelection>,
    environment: Option<&str>,
) -> Result<OcrEngine, String> {
    match selection {
        Some(OcrSelection::Engine(engine)) => Ok(engine),
        Some(OcrSelection::Enabled(false)) => Ok(OcrEngine::Auto),
        _ => environment
            .map(OcrSelection::parse)
            .transpose()
            .map(|s| s.map(OcrSelection::engine).unwrap_or_default()),
    }
}

pub(crate) fn model_revision() -> &'static str {
    weights::REVISION
}

pub fn root() -> Result<PathBuf, String> {
    anymd_formats::cache::cache_dir()
        .map(|p| p.join("models/docvlm-v1"))
        .ok_or_else(|| "Cannot locate anymd cache; set ANYMD_CACHE_DIR".into())
}

/// The one-line message for a VLM request on a machine without the engine or
/// weights. MCP returns it as a normal tool result, like the Pro notice.
pub const SETUP_NOTICE: &str = "VLM OCR needs a one-time setup: run `anymd setup ocr`";

pub fn is_setup_notice(message: &str) -> bool {
    message.contains(SETUP_NOTICE)
}

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
fn companion_intact_at(root: &Path) -> Result<bool, String> {
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
fn refresh_if_stale(
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
fn refreshed_companion(root: &Path) -> Result<(), String> {
    static REFRESH: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    REFRESH.get_or_init(|| install_companion(root)).clone()
}

pub(crate) fn installed_at(root: &Path) -> bool {
    weights::FILES.iter().all(|f| root.join(f.path).is_file())
        && std::fs::read_to_string(root.join("installed"))
            .ok()
            .as_deref()
            == Some(weights::REVISION)
}

pub fn requested(engine: OcrEngine) -> bool {
    if engine == OcrEngine::Vlm {
        return true;
    }
    if engine == OcrEngine::Tesseract {
        return false;
    }
    let mut refresh_error = None;
    if let Ok(root) = root() {
        match refresh_if_stale(&root, &refreshed_companion) {
            Ok(()) if ready_at(&root) && backend_available() => return true,
            Ok(()) => {}
            Err(e) => refresh_error = Some(e),
        }
    }
    static HINT: std::sync::Once = std::sync::Once::new();
    HINT.call_once(|| {
        if let Some(error) = &refresh_error {
            eprintln!("anymd OCR: using tesseract; {error}");
        } else if !anymd_ocr_vlm::hardware::cpu_available() { eprintln!("anymd OCR: using tesseract; doc-VLM needs FP16-capable Linux arm64 hardware."); }
        else { eprintln!("anymd OCR: using tesseract; run `anymd setup ocr` to opt into local doc-VLM OCR (~2 GB weights and the OCR engine)."); }
    });
    // Acceleration alone cannot trigger a model download.
    false
}

/// The hardware can run the engine. Whether the engine is installed is
/// `ready_at`.
pub fn backend_available() -> bool {
    anymd_ocr_vlm::hardware::cpu_available()
}

pub fn metal_available() -> bool {
    #[cfg(feature = "ocr-vlm")]
    {
        anymd_ocr_vlm::candle_backend::CandleBackend::metal_available()
    }
    #[cfg(not(feature = "ocr-vlm"))]
    {
        false
    }
}

fn verified(path: &Path, hash: &str) -> Result<bool, String> {
    Ok(file_digest(path)?.is_some_and(|actual| actual == hash))
}

fn file_digest(path: &Path) -> Result<Option<String>, String> {
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
    Ok(Some(format!("{:x}", digest.finalize())))
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

/// Trusted companion signing keys (base64url raw Ed25519), separate from the
/// Pro licence keys. A list so rotation is additive.
pub const COMPANION_PUBLIC_KEYS: &[&str] = &["zQhktMv7ijbI888ZY9i7xXqExloXuZ-yZUma1ll36j4"];
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

fn install_companion(root: &Path) -> Result<(), String> {
    let platform = companion_platform().ok_or(
        "No VLM OCR engine is published for this platform; tesseract OCR remains available",
    )?;
    let version = env!("CARGO_PKG_VERSION");
    let base = format!("{}/v{version}", crate::RELEASE_DOWNLOAD_BASE);
    install_companion_from(root, &base, platform, version, COMPANION_PUBLIC_KEYS, true)
}

/// `https_only` is false only for tests against a local server.
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

pub fn install() -> Result<PathBuf, String> {
    let root = root()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // The engine is small and fails fastest, so fetch it before ~2 GB of weights.
    if !companion_installed_at(&root) || !companion_intact_at(&root)? {
        install_companion(&root)?;
    }
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
    Ok(root)
}

/// One admitted request, shared by every lazily read PDF page. A failed page
/// poisons this request so callers cannot start a later worker.
pub(crate) struct OcrRequest {
    permit: crate::ocr_evidence::OcrRequestPermit,
    deadline: std::time::Instant,
    failed: Option<String>,
}

impl OcrRequest {
    pub(crate) fn acquire() -> Result<Self, String> {
        Ok(Self {
            permit: crate::ocr_evidence::OcrRequestPermit::acquire()?,
            deadline: std::time::Instant::now()
                + std::time::Duration::from_millis(crate::ocr_evidence::MAX_REQUEST_OCR_TIMEOUT_MS),
            failed: None,
        })
    }

    pub(crate) fn run_page<T>(
        &mut self,
        run: impl FnOnce(
            std::time::Instant,
            &crate::ocr_evidence::OcrRequestPermit,
        ) -> Result<T, String>,
    ) -> Result<T, String> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        let result =
            remaining_page_timeout(self.deadline).and_then(|_| run(self.deadline, &self.permit));
        if let Err(error) = &result {
            self.failed = Some(error.clone());
        }
        result
    }
}

pub(crate) fn remaining_page_timeout(deadline: std::time::Instant) -> Result<u64, String> {
    let remaining = u64::try_from(
        deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    if remaining == 0 {
        return Err("Request exceeds OCR provider time limit; no later worker started".into());
    }
    Ok(page_timeout()?.min(remaining))
}

pub fn recognize(bytes: &[u8]) -> Result<anymd_ocr_vlm::PageResult, String> {
    let mut request = OcrRequest::acquire()?;
    request.run_page(|deadline, permit| recognize_admitted(bytes, deadline, permit))
}

pub(crate) fn recognize_admitted(
    bytes: &[u8],
    deadline: std::time::Instant,
    _permit: &crate::ocr_evidence::OcrRequestPermit,
) -> Result<anymd_ocr_vlm::PageResult, String> {
    let root = root()?;
    if !installed_at(&root) {
        return Err(SETUP_NOTICE.into());
    }
    let command = worker_command()?;
    let timeout = remaining_page_timeout(deadline)?;
    run_worker(&command, bytes, timeout, max_tokens()?)
}

/// The worker protocol: the page image goes in a temp file named in argv, the
/// worker prints one JSON evidence document on stdout.
pub(crate) fn run_worker(
    command: &Path,
    bytes: &[u8],
    timeout: u64,
    tokens: u64,
) -> Result<anymd_ocr_vlm::PageResult, String> {
    let mut input = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    input.write_all(bytes).map_err(|e| e.to_string())?;
    let output = command_provider::run_supervised(CommandInvocation {
        command: command.to_string_lossy().into_owned(),
        args: vec![
            "__ocr-vlm-worker".into(),
            input.path().to_string_lossy().into_owned(),
            tokens.to_string(),
        ],
        timeout_ms: timeout,
        max_stdout_bytes: 2 * 1024 * 1024,
        failure_message: "Doc-VLM worker failed".into(),
        timeout_message: "Doc-VLM page timeout; worker terminated".into(),
    })
    .map_err(|e| e.message)?;
    serde_json::from_str(&output).map_err(|e| format!("Invalid doc-VLM evidence: {e}"))
}

pub(crate) fn max_tokens() -> Result<u64, String> {
    bounded_env("ANYMD_OCR_MAX_TOKENS", 4096, 1, 8192)
}
pub(crate) fn page_timeout() -> Result<u64, String> {
    bounded_env("ANYMD_OCR_TIMEOUT_MS", 300_000, 1_000, 600_000)
}

fn bounded_env(name: &str, default: u64, min: u64, max: u64) -> Result<u64, String> {
    match std::env::var(name) {
        Ok(v) => v
            .parse::<u64>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| format!("{name} must be {min}..{max}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(_) => Err(format!("Invalid {name}")),
    }
}

pub fn worker(arguments: &[String]) -> Result<(), String> {
    let arguments = match arguments
        .last()
        .and_then(|arg| arg.strip_prefix("--supervised="))
    {
        Some(timeout) => {
            let timeout = timeout.parse::<u64>().map_err(|e| e.to_string())?;
            command_provider::supervise_parent(timeout)?;
            &arguments[..arguments.len() - 1]
        }
        None => arguments, // Preserve direct benchmark invocation.
    };
    #[cfg(feature = "ocr-vlm")]
    {
        use anymd_ocr_vlm::DocOcr;
        if !backend_available() {
            return Err(
                "Doc-VLM needs FP16-capable Linux arm64 hardware; tesseract remains available"
                    .into(),
            );
        }
        if arguments.len() != 2 {
            return Err("Invalid OCR worker arguments".into());
        }
        let tokens = arguments[1].parse::<usize>().map_err(|e| e.to_string())?;
        if !(1..=8192).contains(&tokens) {
            return Err("OCR token cap must be 1..8192".into());
        }
        let root = root()?;
        if !installed_at(&root) {
            return Err("OCR models not installed".into());
        }
        let mut reader = image::ImageReader::open(&arguments[0])
            .map_err(|e| e.to_string())?
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16000);
        limits.max_image_height = Some(16000);
        limits.max_alloc = Some(160 * 1024 * 1024);
        reader.limits(limits);
        let page = reader.decode().map_err(|e| e.to_string())?.to_rgb8();
        if u64::from(page.width()) * u64::from(page.height()) > 40_000_000 {
            return Err("OCR page exceeds 40 megapixels".into());
        }
        let quantized = std::env::var("ANYMD_OCR_QUANTIZATION").is_ok_and(|v| v != "none");
        let device = if !quantized && metal_available() {
            "metal"
        } else {
            "cpu"
        };
        let mut backend = anymd_ocr_vlm::candle_backend::CandleBackend::load(
            &root.join("vlm"),
            &root.join("layout"),
            device,
        )
        .map_err(|e| e.to_string())?;
        let mut result = backend
            .recognize_page(&page, tokens)
            .map_err(|e| e.to_string())?;
        format_regions(&mut result)?;
        let mut evidence = serde_json::to_value(&result).map_err(|e| e.to_string())?;
        evidence["text"] = serde_json::json!(result.text());
        evidence["device"] = serde_json::json!(device);
        evidence["model_revision"] = serde_json::json!(weights::REVISION);
        evidence["quantization"] = serde_json::json!(
            std::env::var("ANYMD_OCR_QUANTIZATION").unwrap_or_else(|_| "none".into())
        );
        let mut regions: Vec<_> = result.regions.iter().collect();
        regions.sort_by_key(|r| r.order);
        evidence["words"] = serde_json::json!(regions
            .into_iter()
            .map(|r| serde_json::json!({
                "text": r.text, "reading_order": r.order, "region_type": r.label,
                "layout_confidence": r.score,
                "bounding_box": { "left": r.bbox[0], "bottom": page.height() as f32 - r.bbox[3],
                    "right": r.bbox[2], "top": page.height() as f32 - r.bbox[1] }
            }))
            .collect::<Vec<_>>());
        println!("{}", evidence);
        Ok(())
    }
    #[cfg(not(feature = "ocr-vlm"))]
    {
        let _ = arguments;
        Err("This build has no doc-VLM engine; run `anymd setup ocr` to install it".into())
    }
}

#[cfg_attr(not(feature = "ocr-vlm"), allow(dead_code))]
fn format_regions(result: &mut anymd_ocr_vlm::PageResult) -> Result<(), String> {
    for region in &mut result.regions {
        if region.label.to_ascii_lowercase().contains("table")
            && region.text.to_ascii_lowercase().contains("<table")
        {
            let converted =
                anymd_formats::html::convert(region.text.as_bytes(), &Default::default())
                    .map_err(|e| e.to_string())?;
            region.text = converted
                .sections
                .into_iter()
                .map(|s| s.markdown)
                .collect::<Vec<_>>()
                .join("\n\n");
        } else if region.label.to_ascii_lowercase().contains("formula")
            && !region.text.trim().starts_with('$')
        {
            region.text = format!("$$\n{}\n$$", region.text.trim());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_html_and_formulas_use_the_existing_markdown_converter() {
        use anymd_ocr_vlm::{PageResult, Region};
        let region = |label: &str, text: &str| Region {
            label: label.into(),
            bbox: [0., 0., 10., 10.],
            score: 0.9,
            order: 1,
            text: text.into(),
        };
        let mut result = PageResult {
            regions: vec![
                region(
                    "table",
                    "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
                ),
                region("display_formula", "x^2 + y^2"),
            ],
            truncated: 0,
        };
        format_regions(&mut result).unwrap();
        assert_eq!(result.regions[0].text, "|A|B|\n|-|-|\n|1|2|");
        assert_eq!(result.regions[1].text, "$$\nx^2 + y^2\n$$");
    }

    #[test]
    fn checksum_rejects_a_changed_cache_file() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"hello"));
        assert!(verified(file.path(), &hash).unwrap());
        file.write_all(b" changed").unwrap();
        assert!(!verified(file.path(), &hash).unwrap());
    }

    #[test]
    fn booleans_inherit_the_env_but_named_modes_override_it() {
        assert_eq!(
            resolve_engine(Some(OcrSelection::Enabled(true)), Some("tesseract")).unwrap(),
            OcrEngine::Tesseract
        );
        assert_eq!(resolve_engine(None, Some("vlm")).unwrap(), OcrEngine::Vlm);
        assert_eq!(
            resolve_engine(Some(OcrSelection::Engine(OcrEngine::Auto)), Some("vlm")).unwrap(),
            OcrEngine::Auto
        );
        assert_eq!(
            resolve_engine(Some(OcrSelection::Enabled(false)), Some("invalid")).unwrap(),
            OcrEngine::Auto
        );
        assert!(resolve_engine(None, Some("invalid")).is_err());
    }

    #[test]
    fn selections_preserve_boolean_contract() {
        assert_eq!(
            serde_json::from_str::<OcrSelection>("false").unwrap(),
            OcrSelection::Enabled(false)
        );
        assert_eq!(
            serde_json::from_str::<OcrSelection>("\"vlm\"")
                .unwrap()
                .engine(),
            OcrEngine::Vlm
        );
        assert!(serde_json::from_str::<OcrSelection>("\"remote\"").is_err());
    }
    #[test]
    fn manifest_lookup_needs_the_exact_asset_and_a_full_hash() {
        let hash = "a".repeat(64);
        let manifest = format!(
            "{hash}  anymd-ocr-vlm-linux-x64-gnu\n{} *anymd-ocr-vlm-win32-x64-msvc.exe\nshort  anymd-ocr-vlm-darwin-x64\n",
            "B".repeat(64)
        );
        assert_eq!(
            manifest_hash(&manifest, "anymd-ocr-vlm-linux-x64-gnu"),
            Some(hash)
        );
        assert_eq!(
            manifest_hash(&manifest, "anymd-ocr-vlm-win32-x64-msvc.exe"),
            Some("b".repeat(64))
        );
        assert_eq!(manifest_hash(&manifest, "anymd-ocr-vlm-darwin-x64"), None);
        assert_eq!(manifest_hash(&manifest, "anymd-ocr-vlm-linux-x64"), None);
        assert_eq!(
            companion_asset("win32-x64-msvc"),
            "anymd-ocr-vlm-win32-x64-msvc.exe"
        );
        assert_eq!(
            companion_asset("darwin-arm64"),
            "anymd-ocr-vlm-darwin-arm64"
        );
    }

    #[cfg(not(feature = "ocr-vlm"))]
    #[test]
    fn missing_or_stale_companion_gives_the_setup_notice() {
        let no_refresh = |_: &Path| Err::<(), String>("offline".into());
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            worker_command_with(dir.path(), &no_refresh),
            Err("VLM OCR needs a one-time setup: run `anymd setup ocr`".to_string())
        );
        // Weights alone are not enough, and neither is a companion from another version.
        for file in weights::FILES {
            let path = dir.path().join(file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        std::fs::write(dir.path().join("installed"), weights::REVISION).unwrap();
        assert!(installed_at(dir.path()));
        assert!(worker_command_with(dir.path(), &no_refresh).is_err());
        std::fs::write(dir.path().join(companion_file_name()), b"x").unwrap();
        std::fs::write(
            dir.path().join(COMPANION_HASH_FILE),
            format!("{:x}", Sha256::digest(b"x")),
        )
        .unwrap();
        std::fs::write(dir.path().join(COMPANION_VERSION_FILE), "0.0.0").unwrap();
        assert!(worker_command_with(dir.path(), &no_refresh).is_err());
        std::fs::write(
            dir.path().join(COMPANION_VERSION_FILE),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        assert_eq!(
            worker_command_with(dir.path(), &no_refresh).unwrap(),
            dir.path().join(companion_file_name())
        );
    }

    /// The companion protocol with a stub engine: argv carries the worker
    /// name, the page-image path and the token cap; stdout carries the JSON.
    #[cfg(unix)]
    #[test]
    fn stub_companion_round_trips_a_page() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("anymd-ocr-vlm");
        std::fs::write(
            &stub,
            r#"#!/bin/sh
[ "$1" = "__ocr-vlm-worker" ] || exit 3
[ "$(cat "$2")" = "page-bytes" ] || exit 4
[ "$3" = "17" ] || exit 5
printf '{"regions":[{"label":"text","bbox":[0,0,1,1],"score":0.9,"order":2,"text":"world"},{"label":"text","bbox":[0,0,1,1],"score":0.9,"order":1,"text":"hello"}],"truncated":0,"device":"cpu"}'
"#,
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let page = run_worker(&stub, b"page-bytes", 10_000, 17).unwrap();
        assert_eq!(page.markdown(), "hello\n\nworld");
        // A worker that exits non-zero is an error, not a page.
        assert!(run_worker(&stub, b"other-bytes", 10_000, 17).is_err());
    }

    #[test]
    fn incomplete_cache_is_not_an_opt_in() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("installed"), weights::REVISION).unwrap();
        assert!(!installed_at(dir.path()));
    }
    #[test]
    fn all_weights_have_full_hashes_and_immutable_urls() {
        for file in weights::FILES {
            assert_eq!(file.sha256.len(), 64);
            assert!(file.url.starts_with("https://huggingface.co/"));
            assert!(!file.url.contains("/main/"));
            assert!(file.bytes > 0);
        }
    }

    // ---- Signed companion manifest ----

    use ed25519_dalek::{Signer, SigningKey};

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }
    fn public_key(key: &SigningKey) -> String {
        URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes())
    }
    fn sign(key: &SigningKey, version: &str, manifest: &str) -> String {
        URL_SAFE_NO_PAD.encode(key.sign(&signed_message(version, manifest)).to_bytes())
    }

    #[test]
    fn manifest_signature_binds_the_version_and_the_bytes() {
        let key = signing_key();
        let keys = [public_key(&key)];
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        let manifest = "aa  anymd-ocr-vlm-x\n";
        let sig = sign(&key, "1.2.3", manifest);
        assert!(verify_manifest("1.2.3", manifest, &sig, &keys).is_ok());
        // A tampered manifest.
        assert!(verify_manifest("1.2.3", "bb  anymd-ocr-vlm-x\n", &sig, &keys).is_err());
        // A signature from an older release.
        let old = sign(&key, "1.2.2", manifest);
        assert!(verify_manifest("1.2.3", manifest, &old, &keys).is_err());
        // A signature by another key, and a malformed one.
        let other = sign(&SigningKey::from_bytes(&[8; 32]), "1.2.3", manifest);
        assert!(verify_manifest("1.2.3", manifest, &other, &keys).is_err());
        assert!(verify_manifest("1.2.3", manifest, "not-a-signature", &keys).is_err());
        // The compiled-in list holds valid public keys.
        for key in COMPANION_PUBLIC_KEYS {
            let raw = URL_SAFE_NO_PAD.decode(key).unwrap();
            assert!(VerifyingKey::from_bytes(&<[u8; 32]>::try_from(raw).unwrap()).is_ok());
        }
    }

    /// A one-thread HTTP server over fixed `(path, body)` pairs; anything else is 404.
    fn serve(files: Vec<(String, Vec<u8>)>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    request.push(byte[0]);
                }
                let line = String::from_utf8_lossy(&request);
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let (status, body) = match files.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => ("200 OK", body.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{address}")
    }

    #[cfg(unix)]
    struct Release {
        base: String,
    }

    /// A release for `platform` whose companion script prints `reported`; the
    /// manifest is signed for `signed_version` and then optionally tampered with.
    #[cfg(unix)]
    fn release(key: &SigningKey, signed_version: &str, reported: &str, tamper: bool) -> Release {
        let asset = companion_asset("test-plat");
        let binary = format!("#!/bin/sh\necho 'anymd-ocr-vlm {reported}'\n").into_bytes();
        let manifest = format!("{:x}  {asset}\n", Sha256::digest(&binary));
        let signature = sign(key, signed_version, &manifest);
        let served = if tamper {
            format!("{:x}  {asset}\n", Sha256::digest(b"evil"))
        } else {
            manifest
        };
        let base = serve(vec![
            ("/v/anymd-ocr-vlm-SHA256SUMS".into(), served.into_bytes()),
            (
                "/v/anymd-ocr-vlm-SHA256SUMS.sig".into(),
                signature.into_bytes(),
            ),
            (format!("/v/{asset}"), binary),
        ]);
        Release {
            base: format!("{base}/v"),
        }
    }

    #[cfg(unix)]
    fn install_from(release: &Release, root: &Path, key: &SigningKey) -> Result<(), String> {
        install_companion_from(
            root,
            &release.base,
            "test-plat",
            env!("CARGO_PKG_VERSION"),
            &[&public_key(key)],
            false,
        )
    }

    #[cfg(unix)]
    #[test]
    fn signed_matching_companion_installs() {
        let key = signing_key();
        let version = env!("CARGO_PKG_VERSION");
        let dir = tempfile::tempdir().unwrap();
        let ok = release(&key, version, version, false);
        install_from(&ok, dir.path(), &key).unwrap();
        assert!(companion_installed_at(dir.path()) || cfg!(feature = "ocr-vlm"));
        assert!(dir.path().join(companion_file_name()).is_file());
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_companions_refuse_to_install() {
        let key = signing_key();
        let version = env!("CARGO_PKG_VERSION");
        let refuses = |release: Release, key: &SigningKey| {
            let dir = tempfile::tempdir().unwrap();
            let result = install_from(&release, dir.path(), key);
            assert!(result.is_err(), "installed: {result:?}");
            assert!(!dir.path().join(companion_file_name()).exists());
            assert!(!dir.path().join(COMPANION_VERSION_FILE).exists());
            result.unwrap_err()
        };
        // Tampered SHA256SUMS.
        refuses(release(&key, version, version, true), &key);
        // Signature from an older version.
        refuses(release(&key, "0.0.1", version, false), &key);
        // Signature by a key that is not trusted.
        refuses(
            release(&SigningKey::from_bytes(&[9; 32]), version, version, false),
            &key,
        );
        // The binary reports another version.
        let error = refuses(release(&key, version, "0.0.1", false), &key);
        assert!(error.contains("does not report version"));
        // Nothing published yet.
        let dir = tempfile::tempdir().unwrap();
        let empty = Release {
            base: format!("{}/v", serve(Vec::new())),
        };
        let error = install_from(&empty, dir.path(), &key).unwrap_err();
        assert!(error.contains("not published yet; retry later"), "{error}");
        // Plain http is refused by the production agent.
        let ok = release(&key, version, version, false);
        let error = install_companion_from(
            dir.path(),
            &ok.base,
            "test-plat",
            version,
            &[&public_key(&key)],
            true,
        );
        assert!(error.is_err());
    }

    /// Weights installed, engine from an older version: the engine is
    /// refreshed once and VLM stays on. A failed refresh falls back with a hint.
    #[cfg(all(unix, not(feature = "ocr-vlm")))]
    #[test]
    fn upgrade_refreshes_the_engine_when_weights_are_installed() {
        let key = signing_key();
        let version = env!("CARGO_PKG_VERSION");
        let dir = tempfile::tempdir().unwrap();
        for file in weights::FILES {
            let path = dir.path().join(file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        std::fs::write(dir.path().join("installed"), weights::REVISION).unwrap();
        std::fs::write(dir.path().join(companion_file_name()), b"old").unwrap();
        std::fs::write(dir.path().join(COMPANION_VERSION_FILE), "0.0.0").unwrap();
        assert!(!ready_at(dir.path()));

        let down = Release {
            base: format!("{}/v", serve(Vec::new())),
        };
        let error =
            worker_command_with(dir.path(), &|root| install_from(&down, root, &key)).unwrap_err();
        assert!(error.contains("could not be refreshed"), "{error}");
        assert!(error.ends_with("run anymd setup ocr"), "{error}");

        let ok = release(&key, version, version, false);
        let command =
            worker_command_with(dir.path(), &|root| install_from(&ok, root, &key)).unwrap();
        assert_eq!(command, dir.path().join(companion_file_name()));
        assert!(ready_at(dir.path()));
        // Missing weights never trigger a download.
        let bare = tempfile::tempdir().unwrap();
        let result = worker_command_with(bare.path(), &|_| panic!("must not refresh"));
        assert_eq!(result, Err(SETUP_NOTICE.to_string()));
    }

    fn weights_in(root: &Path) {
        for file in weights::FILES {
            let path = root.join(file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        std::fs::write(root.join("installed"), weights::REVISION).unwrap();
    }

    fn companion_in(root: &Path, bytes: &[u8]) {
        std::fs::write(root.join(companion_file_name()), bytes).unwrap();
        std::fs::write(
            root.join(COMPANION_HASH_FILE),
            format!("{:x}", Sha256::digest(bytes)),
        )
        .unwrap();
        std::fs::write(root.join(COMPANION_VERSION_FILE), env!("CARGO_PKG_VERSION")).unwrap();
    }

    #[cfg(not(feature = "ocr-vlm"))]
    #[test]
    fn a_matching_companion_runs_and_a_tampered_one_never_does() {
        let dir = tempfile::tempdir().unwrap();
        weights_in(dir.path());
        companion_in(dir.path(), b"engine");
        let must_not_refresh = |_: &Path| -> Result<(), String> { panic!("must not refresh") };
        assert_eq!(
            worker_command_with(dir.path(), &must_not_refresh).unwrap(),
            dir.path().join(companion_file_name())
        );
        // Same size, so only the content hash can tell.
        std::fs::write(dir.path().join(companion_file_name()), b"engind").unwrap();
        // Refresh once, then run only if it is repaired.
        let calls = std::cell::Cell::new(0);
        let broken = |_: &Path| -> Result<(), String> {
            calls.set(calls.get() + 1);
            Ok(())
        };
        assert_eq!(
            worker_command_with(dir.path(), &broken),
            Err(INTEGRITY_NOTICE.to_string())
        );
        assert_eq!(calls.get(), 1);
        let repair = |root: &Path| -> Result<(), String> {
            companion_in(root, b"engine!");
            Ok(())
        };
        assert_eq!(
            worker_command_with(dir.path(), &repair).unwrap(),
            dir.path().join(companion_file_name())
        );
    }

    #[cfg(not(feature = "ocr-vlm"))]
    #[test]
    fn a_missing_hash_stamp_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        weights_in(dir.path());
        companion_in(dir.path(), b"engine");
        std::fs::remove_file(dir.path().join(COMPANION_HASH_FILE)).unwrap();
        let result = worker_command_with(dir.path(), &|_| Ok(()));
        assert_eq!(result, Err(INTEGRITY_NOTICE.to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn install_stores_the_verified_hash() {
        let key = signing_key();
        let version = env!("CARGO_PKG_VERSION");
        let dir = tempfile::tempdir().unwrap();
        let ok = release(&key, version, version, false);
        install_from(&ok, dir.path(), &key).unwrap();
        let stored = std::fs::read_to_string(dir.path().join(COMPANION_HASH_FILE)).unwrap();
        let actual = file_digest(&dir.path().join(companion_file_name()))
            .unwrap()
            .unwrap();
        assert_eq!(stored, actual);
        assert!(companion_intact_at(dir.path()).unwrap());
    }
}
