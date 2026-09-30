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

pub fn root() -> Result<PathBuf, String> {
    anymd_formats::cache::cache_dir()
        .map(|p| p.join("models/docvlm-v1"))
        .ok_or_else(|| "Cannot locate anymd cache; set ANYMD_CACHE_DIR".into())
}

fn installed_at(root: &Path) -> bool {
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
    let installed = root().is_ok_and(|p| installed_at(&p));
    if installed && backend_available() {
        return true;
    }
    static HINT: std::sync::Once = std::sync::Once::new();
    HINT.call_once(|| eprintln!("anymd OCR: using tesseract; run `anymd setup ocr` to opt into local doc-VLM OCR (~2 GB weights)."));
    // Acceleration alone cannot trigger a model download.
    false
}

pub fn backend_available() -> bool {
    if !cfg!(feature = "ocr-vlm") {
        return false;
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        return std::arch::is_aarch64_feature_detected!("fp16");
    }
    #[cfg(not(all(target_arch = "aarch64", target_os = "linux")))]
    {
        true
    }
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

pub fn install() -> Result<PathBuf, String> {
    if !cfg!(feature = "ocr-vlm") {
        return Err("This build has no doc-VLM backend".into());
    }
    let root = root()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
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

pub fn recognize(bytes: &[u8]) -> Result<anymd_ocr_vlm::PageResult, String> {
    let root = root()?;
    if !installed_at(&root) {
        return Err("Doc-VLM weights are not installed; run `anymd setup ocr`".into());
    }
    let mut input = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    input.write_all(bytes).map_err(|e| e.to_string())?;
    let timeout = bounded_env("ANYMD_OCR_TIMEOUT_MS", 300_000, 1_000, 600_000)?;
    let tokens = bounded_env("ANYMD_OCR_MAX_TOKENS", 4096, 1, 8192)?;
    let command = std::env::current_exe().map_err(|e| e.to_string())?;
    let output = command_provider::run(CommandInvocation {
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
        for region in &mut result.regions {
            if region.label.to_ascii_lowercase().contains("table") && region.text.contains("<table")
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
        let mut evidence = serde_json::to_value(&result).map_err(|e| e.to_string())?;
        evidence["text"] = serde_json::json!(result.text());
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
        Err("This build has no doc-VLM backend".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
