//! Bound page execution and implement the local worker protocol.
#[cfg(feature = "ocr-vlm")]
use super::{backend_available, installed_at, metal_available, root, weights};
use crate::command_provider::{self, CommandInvocation};
use std::io::Write;
use std::path::Path;

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

#[cfg(any(feature = "ocr-vlm", test))]
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
mod tests;
