//! Explicit model setup and bounded local doc-VLM worker.
use crate::command_provider::{self, CommandInvocation};
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
    if !ready_at(root) {
        return Err(SETUP_NOTICE.into());
    }
    Ok(root.join(companion_file_name()))
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
    let ready = root().is_ok_and(|p| ready_at(&p));
    if ready && backend_available() {
        return true;
    }
    static HINT: std::sync::Once = std::sync::Once::new();
    HINT.call_once(|| {
        if !anymd_ocr_vlm::hardware::cpu_available() { eprintln!("anymd OCR: using tesseract; doc-VLM needs FP16-capable Linux arm64 hardware."); }
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
    let Ok(mut file) = std::fs::File::open(path) else {
        return Ok(false);
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
    Ok(format!("{:x}", digest.finalize()) == hash)
}

/// The release-asset platform key (the npm package suffix) of this build.
pub fn companion_platform() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("darwin-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("darwin-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")) {
        Some("linux-x64-gnu")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu")) {
        Some("linux-arm64-gnu")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("win32-x64-msvc")
    } else {
        None
    }
}

/// Release asset name of the companion for `platform`.
pub fn companion_asset(platform: &str) -> String {
    let suffix = if platform.starts_with("win32") { ".exe" } else { "" };
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

fn install_companion(root: &Path) -> Result<(), String> {
    let platform = companion_platform().ok_or(
        "No VLM OCR engine is published for this platform; tesseract OCR remains available",
    )?;
    let version = env!("CARGO_PKG_VERSION");
    let base = format!("{}/v{version}", crate::RELEASE_DOWNLOAD_BASE);
    let asset = companion_asset(platform);
    eprintln!("Downloading the OCR engine {asset}");
    let mut manifest = String::new();
    ureq::get(&format!("{base}/{COMPANION_MANIFEST}"))
        .call()
        .map_err(|e| format!("OCR engine manifest: {e}"))?
        .body_mut()
        .as_reader()
        .take(1024 * 1024)
        .read_to_string(&mut manifest)
        .map_err(|e| e.to_string())?;
    let hash = manifest_hash(&manifest, &asset)
        .ok_or_else(|| format!("OCR engine manifest does not list {asset}"))?;
    let mut response = ureq::get(&format!("{base}/{asset}"))
        .call()
        .map_err(|e| format!("OCR engine download: {e}"))?;
    let mut reader = response.body_mut().as_reader().take(COMPANION_MAX_BYTES + 1);
    let mut temporary = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
    let n = std::io::copy(&mut reader, &mut temporary).map_err(|e| e.to_string())?;
    temporary.flush().map_err(|e| e.to_string())?;
    if n > COMPANION_MAX_BYTES || !verified(temporary.path(), &hash)? {
        return Err(format!("OCR engine hash mismatch: {asset}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    temporary
        .persist(root.join(companion_file_name()))
        .map_err(|e| e.to_string())?;
    // The version file is what makes the engine count as installed.
    std::fs::write(root.join(COMPANION_VERSION_FILE), version).map_err(|e| e.to_string())
}

pub fn install() -> Result<PathBuf, String> {
    let root = root()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // The engine is small and fails fastest, so fetch it before ~2 GB of weights.
    if !companion_installed_at(&root) {
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
    if !ready_at(&root) {
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
        assert_eq!(companion_asset("win32-x64-msvc"), "anymd-ocr-vlm-win32-x64-msvc.exe");
        assert_eq!(companion_asset("darwin-arm64"), "anymd-ocr-vlm-darwin-arm64");
    }

    #[cfg(not(feature = "ocr-vlm"))]
    #[test]
    fn missing_or_stale_companion_gives_the_setup_notice() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            worker_command_at(dir.path()),
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
        assert!(worker_command_at(dir.path()).is_err());
        std::fs::write(dir.path().join(companion_file_name()), b"x").unwrap();
        std::fs::write(dir.path().join(COMPANION_VERSION_FILE), "0.0.0").unwrap();
        assert!(worker_command_at(dir.path()).is_err());
        std::fs::write(dir.path().join(COMPANION_VERSION_FILE), env!("CARGO_PKG_VERSION")).unwrap();
        assert_eq!(
            worker_command_at(dir.path()).unwrap(),
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
}
