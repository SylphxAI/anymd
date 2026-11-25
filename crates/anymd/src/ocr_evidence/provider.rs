//! OCR provider selection, command configuration, and invocation.

use std::env;

use serde_json::{json, Value};
use tempfile::TempDir;

use crate::command_provider::{self, CommandInvocation, CommandRunError};

pub(super) const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const OCR_COMMAND_ENV: &str = "MCP_PDF_OCR_COMMAND";
const OCR_ARGS_ENV: &str = "MCP_PDF_OCR_ARGS_JSON";
const OCR_PRESET_ENV: &str = "MCP_PDF_OCR_PRESET";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OcrOutputFormat {
    Auto,
    TesseractTsv,
}

#[derive(Clone)]
pub(super) struct ProviderConfig {
    pub(super) command: String,
    pub(super) args_template: Vec<String>,
    pub(super) output_format: OcrOutputFormat,
}

impl ProviderConfig {
    pub(super) fn builtin_vlm(&self) -> bool {
        self.args_template
            .first()
            .is_some_and(|arg| arg == "__ocr-vlm-worker")
    }

    pub(super) fn page_timeout(&self, requested: Option<u64>) -> Result<u64, String> {
        requested.map_or_else(
            || {
                if self.builtin_vlm() {
                    crate::ocr_vlm::page_timeout()
                } else {
                    Ok(DEFAULT_TIMEOUT_MS)
                }
            },
            Ok,
        )
    }

    pub(super) fn stamp_provenance(&self, output: &mut Value) {
        let mut provenance = output
            .get("provenance")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if self.builtin_vlm() {
            provenance.insert("engine".into(), json!("candle"));
            provenance.insert("source".into(), json!("paddleocr-vl-1.6"));
            provenance.insert("layout".into(), json!("pp-doclayoutv3"));
        } else {
            provenance.insert("engine".into(), json!("external-command"));
            provenance.insert("source".into(), json!("ocr-provider"));
        }
        output["provenance"] = json!(provenance);
    }
}

pub(super) fn provider_config_from(
    command_value: Option<String>,
    preset_value: Option<String>,
    args_value: Option<String>,
) -> Result<ProviderConfig, String> {
    let preset = preset_value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if preset
        .as_deref()
        .is_some_and(|value| !matches!(value, "tesseract" | "tesseract-tsv"))
    {
        return Err(
            "Unsupported MCP_PDF_OCR_PRESET. Supported values: tesseract, tesseract-tsv.".into(),
        );
    }
    let command = command_value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| preset.as_ref().map(|_| "tesseract".into()))
        .ok_or_else(|| {
            "OCR provider is not configured. Set MCP_PDF_OCR_COMMAND or MCP_PDF_OCR_PRESET=tesseract to enable ocr_pages."
                .to_string()
        })?;
    let default_args = match preset.as_deref() {
        Some("tesseract-tsv") => vec![
            "{input}".into(),
            "stdout".into(),
            "-l".into(),
            "{languages_tesseract}".into(),
            "tsv".into(),
        ],
        Some("tesseract") => vec![
            "{input}".into(),
            "stdout".into(),
            "-l".into(),
            "{languages_tesseract}".into(),
        ],
        _ => vec!["{input}".into()],
    };
    let args_template = match args_value {
        None => default_args,
        Some(raw) => {
            let value: Value = serde_json::from_str(&raw)
                .map_err(|_| "MCP_PDF_OCR_ARGS_JSON must be a JSON string array.".to_string())?;
            let array = value
                .as_array()
                .ok_or_else(|| "MCP_PDF_OCR_ARGS_JSON must be a JSON string array.".to_string())?;
            let args = array
                .iter()
                .map(|entry| {
                    entry.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                        "MCP_PDF_OCR_ARGS_JSON must be a JSON string array.".to_string()
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if !args.iter().any(|arg| arg.contains("{input}")) {
                return Err("MCP_PDF_OCR_ARGS_JSON must include the {input} placeholder so the OCR provider receives the rendered page image.".into());
            }
            args
        }
    };
    let output_format = match preset.as_deref() {
        Some("tesseract-tsv") => OcrOutputFormat::TesseractTsv,
        _ => OcrOutputFormat::Auto,
    };
    Ok(ProviderConfig {
        command,
        args_template,
        output_format,
    })
}

pub(super) fn provider_config(
    engine: Option<crate::ocr_vlm::OcrEngine>,
) -> Result<ProviderConfig, String> {
    let selected = match engine {
        Some(engine) => Some(engine),
        None if env::var(OCR_COMMAND_ENV).is_err() => env::var("ANYMD_OCR")
            .ok()
            .map(|value| crate::ocr_vlm::OcrSelection::parse(&value).map(|s| s.engine()))
            .transpose()?,
        None => None,
    };
    if let Some(engine) = selected {
        if crate::ocr_vlm::requested(engine) {
            let command = crate::ocr_vlm::worker_command()?;
            return Ok(ProviderConfig {
                command: command.to_string_lossy().into_owned(),
                args_template: vec![
                    "__ocr-vlm-worker".into(),
                    "{input}".into(),
                    crate::ocr_vlm::max_tokens()?.to_string(),
                ],
                output_format: OcrOutputFormat::Auto,
            });
        }
        return provider_config_from(None, Some("tesseract-tsv".into()), None);
    }
    provider_config_from(
        env::var(OCR_COMMAND_ENV).ok(),
        env::var(OCR_PRESET_ENV).ok(),
        env::var(OCR_ARGS_ENV).ok(),
    )
}

fn replace_placeholders(
    template: &str,
    input: &str,
    page: u64,
    source: &str,
    languages: &[String],
) -> String {
    template
        .replace("{input}", input)
        .replace("{page}", &page.to_string())
        .replace("{source}", source)
        .replace("{language}", languages.first().map_or("", String::as_str))
        .replace("{languages}", &languages.join(","))
        .replace(
            "{languages_tesseract}",
            if languages.is_empty() {
                "eng".into()
            } else {
                languages.join("+")
            }
            .as_str(),
        )
}

pub(super) fn run_provider(
    config: &ProviderConfig,
    png: &[u8],
    page: u64,
    source: &str,
    languages: &[String],
    timeout_ms: u64,
    max_output_chars: usize,
) -> Result<String, CommandRunError> {
    let temp = TempDir::with_prefix("pdf-reader-mcp-ocr-")
        .map_err(|_| CommandRunError::new("Failed to create OCR provider workspace.".into(), 0))?;
    let input = temp.path().join(format!("page-{page}.png"));
    std::fs::write(&input, png)
        .map_err(|_| CommandRunError::new("Failed to write OCR provider input image.".into(), 0))?;
    let input = input.to_string_lossy();
    let args = config
        .args_template
        .iter()
        .map(|arg| replace_placeholders(arg, &input, page, source, languages))
        .collect::<Vec<_>>();
    let maximum = max_output_chars.saturating_mul(4).max(1024 * 1024);
    command_provider::run(CommandInvocation {
        command: config.command.clone(),
        args,
        timeout_ms,
        max_stdout_bytes: maximum,
        failure_message: format!("OCR provider command failed for page {page}."),
        timeout_message: format!("OCR provider command timed out for page {page}."),
    })
}
