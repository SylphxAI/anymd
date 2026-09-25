//! Optional command-provider analysis over bounded pure-Rust region crops.

use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Map, Value};
use tempfile::TempDir;

use crate::command_provider::{self, CommandInvocation, CommandRunError};
use crate::schema::PdfEvidenceArgs;
use crate::visual_evidence;

mod normalize;
mod provider_config;

use normalize::*;
use provider_config::*;

const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_MAX_OUTPUT_CHARS: usize = 200_000;
const COMMAND_ENV: &str = "MCP_PDF_REGION_ANALYSIS_COMMAND";
const ARGS_ENV: &str = "MCP_PDF_REGION_ANALYSIS_ARGS_JSON";
const HTTP_ENV: &str = "MCP_PDF_REGION_ANALYSIS_HTTP_URL";
const HTTP_HEADERS_ENV: &str = "MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON";
const PRESET_ENV: &str = "MCP_PDF_REGION_ANALYSIS_PRESET";
const OLLAMA_URL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OLLAMA_URL";
const OLLAMA_MODEL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OLLAMA_MODEL";
const OPENAI_URL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OPENAI_URL";
const OPENAI_MODEL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OPENAI_MODEL";
const OPENAI_API_KEY_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OPENAI_API_KEY";
const LMSTUDIO_URL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_LMSTUDIO_URL";
const LMSTUDIO_MODEL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_LMSTUDIO_MODEL";
const LLAMACPP_URL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_LLAMACPP_URL";
const LLAMACPP_MODEL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_LLAMACPP_MODEL";
const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434/api/generate";
const DEFAULT_LMSTUDIO_URL: &str = "http://127.0.0.1:1234/v1/chat/completions";
const DEFAULT_LLAMACPP_URL: &str = "http://127.0.0.1:8080/v1/chat/completions";
const MAX_SOURCES_PER_REQUEST: usize = 32;
const MAX_REQUEST_TIMEOUT_MS: u64 = 600_000;
const MAX_REQUEST_OUTPUT_CHARS: usize = 2_000_000;
const MAX_REQUEST_PROVIDER_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONCURRENT_REQUESTS: usize = 2;
static ACTIVE_REQUESTS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct RequestPermit;

impl RequestPermit {
    fn acquire() -> Result<Self, String> {
        ACTIVE_REQUESTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_CONCURRENT_REQUESTS).then_some(active + 1)
            })
            .map(|_| Self)
            .map_err(|_| {
                format!(
                    "Region analysis provider concurrency limit of {MAX_CONCURRENT_REQUESTS} active requests reached; retry later."
                )
            })
    }
}

impl Drop for RequestPermit {
    fn drop(&mut self) {
        ACTIVE_REQUESTS.fetch_sub(1, Ordering::Release);
    }
}

#[derive(Default)]
struct RequestBudget {
    provider_bytes: usize,
    output_chars: usize,
    exhausted: Option<String>,
}

impl RequestBudget {
    fn ensure_available(&self) -> Result<(), String> {
        self.exhausted.clone().map_or(Ok(()), Err)
    }

    fn exhaust(&mut self, message: String) -> Result<(), String> {
        self.exhausted = Some(message.clone());
        Err(message)
    }

    fn charge(&mut self, provider_bytes: usize, output_chars: usize) -> Result<(), String> {
        self.ensure_available()?;
        self.provider_bytes = match self.provider_bytes.checked_add(provider_bytes) {
            Some(value) => value,
            None => {
                return self.exhaust("Request region analysis provider byte count overflow.".into())
            }
        };
        if self.provider_bytes > MAX_REQUEST_PROVIDER_BYTES {
            return self.exhaust(format!(
                "Request exceeds region analysis provider output limit of {MAX_REQUEST_PROVIDER_BYTES} bytes."
            ));
        }
        self.output_chars = match self.output_chars.checked_add(output_chars) {
            Some(value) => value,
            None => {
                return self
                    .exhaust("Request region analysis output character count overflow.".into())
            }
        };
        if self.output_chars > MAX_REQUEST_OUTPUT_CHARS {
            return self.exhaust(format!(
                "Request exceeds region analysis output limit of {MAX_REQUEST_OUTPUT_CHARS} characters."
            ));
        }
        Ok(())
    }

    fn charge_failed_provider(&mut self, provider_bytes: usize) -> Result<(), String> {
        self.charge(provider_bytes, 0)
    }
}

#[derive(Clone, Debug)]
enum ProviderConfig {
    Command {
        command: String,
        args_template: Vec<String>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        preset: Option<String>,
        model: Option<String>,
    },
}

impl ProviderConfig {
    fn provider_label(&self) -> &'static str {
        match self {
            Self::Command { .. } => "command",
            Self::Http { .. } => "http",
        }
    }

    fn engine_label(&self) -> &'static str {
        match self {
            Self::Command { .. } => "external-command",
            Self::Http { .. } => "external-http",
        }
    }
}

fn parse_args(value: &Value) -> Result<PdfEvidenceArgs, rmcp::ErrorData> {
    let args: PdfEvidenceArgs = serde_json::from_value(value.clone()).map_err(|error| {
        rmcp::ErrorData::invalid_params(format!("Invalid pdf_evidence arguments: {error}"), None)
    })?;
    args.validate()
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    Ok(args)
}

fn js_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn replace_placeholders(
    template: &str,
    input: &str,
    source: &str,
    region: &Value,
    languages: &[String],
) -> String {
    let box_ = &region["source_bounding_box"];
    template
        .replace("{input}", input)
        .replace("{page}", &region["page"].as_u64().unwrap_or(0).to_string())
        .replace("{source}", source)
        .replace("{region_id}", region["region_id"].as_str().unwrap_or(""))
        .replace(
            "{evidence_id}",
            region["evidence_id"].as_str().unwrap_or(""),
        )
        .replace("{left}", &js_number(box_["left"].as_f64().unwrap_or(0.0)))
        .replace(
            "{bottom}",
            &js_number(box_["bottom"].as_f64().unwrap_or(0.0)),
        )
        .replace("{right}", &js_number(box_["right"].as_f64().unwrap_or(0.0)))
        .replace("{top}", &js_number(box_["top"].as_f64().unwrap_or(0.0)))
        .replace("{language}", languages.first().map_or("", String::as_str))
        .replace("{languages}", &languages.join(","))
}

fn vision_prompt(region: &Value, source: &str, languages: &[String]) -> String {
    let box_ = &region["source_bounding_box"];
    format!(
        "Analyze this cropped PDF region for an AI document parser.\nSource: {source}\nRegion id: {}\nPage: {}\nPDF bounding box: left={}, bottom={}, right={}, top={}\nLanguages: {}\nReturn only one JSON object with kind/description/table/formula/chart fields when applicable.",
        region["region_id"].as_str().unwrap_or(""),
        region["page"].as_u64().unwrap_or(0),
        js_number(box_["left"].as_f64().unwrap_or(0.0)),
        js_number(box_["bottom"].as_f64().unwrap_or(0.0)),
        js_number(box_["right"].as_f64().unwrap_or(0.0)),
        js_number(box_["top"].as_f64().unwrap_or(0.0)),
        if languages.is_empty() {
            "unspecified".into()
        } else {
            languages.join(",")
        }
    )
}

fn build_http_request_body(
    config: &ProviderConfig,
    image_b64: &str,
    mime_type: &str,
    source: &str,
    region: &Value,
    languages: &[String],
) -> Result<Value, String> {
    let ProviderConfig::Http {
        preset,
        model,
        ..
    } = config
    else {
        return Err("Internal error: HTTP body builder called for non-HTTP provider.".into());
    };
    match preset.as_deref() {
        Some("ollama") => Ok(json!({
            "model": model,
            "prompt": vision_prompt(region, source, languages),
            "images": [image_b64],
            "stream": false,
            "format": "json",
        })),
        Some("openai-compatible" | "lmstudio" | "llamacpp") => Ok(json!({
            "model": model,
            "messages": [
                {
                    "role": "system",
                    "content": "You analyze cropped PDF regions for an AI document parser. Return only one JSON object."
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": vision_prompt(region, source, languages)},
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{mime_type};base64,{image_b64}")
                            }
                        }
                    ]
                }
            ],
            "temperature": 0
        })),
        _ => Ok(json!({
            "image_base64": image_b64,
            "mime_type": mime_type,
            "format": region.get("format").cloned().unwrap_or_else(|| json!("png")),
            "page": region.get("page").cloned().unwrap_or(json!(0)),
            "region_id": region.get("region_id").cloned().unwrap_or(json!("")),
            "evidence_id": region.get("evidence_id").cloned().unwrap_or(json!("")),
            "source": source,
            "source_bounding_box": region.get("source_bounding_box").cloned().unwrap_or(json!({})),
            "crop_pixels": region.get("crop_pixels").cloned().unwrap_or(json!({})),
            "scale": region.get("scale").cloned().unwrap_or(json!(1.0)),
            "languages": languages,
        })),
    }
}

fn parse_http_provider_stdout(preset: Option<&str>, stdout: &str) -> Result<String, String> {
    match preset {
        Some("ollama") => {
            let parsed: Value = serde_json::from_str(stdout)
                .map_err(|_| "Ollama region analysis response was not a JSON object.".to_string())?;
            let response = parsed
                .get("response")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "Ollama region analysis response did not include a non-empty response string."
                        .to_string()
                })?;
            Ok(response.to_string())
        }
        Some("openai-compatible" | "lmstudio" | "llamacpp") => {
            let parsed: Value = serde_json::from_str(stdout).map_err(|_| {
                "OpenAI-compatible region analysis response was not a JSON object.".to_string()
            })?;
            let content = parsed
                .pointer("/choices/0/message/content")
                .cloned()
                .ok_or_else(|| {
                    "OpenAI-compatible region analysis response did not include message content."
                        .to_string()
                })?;
            if let Some(text) = content.as_str().map(str::trim).filter(|value| !value.is_empty()) {
                return Ok(text.to_string());
            }
            if let Some(parts) = content.as_array() {
                let text = parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return Ok(text);
                }
            }
            Err(
                "OpenAI-compatible region analysis response did not include message content."
                    .into(),
            )
        }
        _ => Ok(stdout.to_string()),
    }
}

fn run_http_provider(
    config: &ProviderConfig,
    png: &[u8],
    source: &str,
    region: &Value,
    languages: &[String],
    timeout_ms: u64,
) -> Result<String, CommandRunError> {
    let ProviderConfig::Http {
        url,
        headers,
        preset,
        ..
    } = config
    else {
        return Err(CommandRunError::new(
            "Internal error: HTTP provider invoked for non-HTTP config.".into(),
            0,
        ));
    };
    let page = region["page"].as_u64().unwrap_or(0);
    let region_id = region["region_id"].as_str().unwrap_or("unknown");
    let mime_type = region["mime_type"].as_str().unwrap_or("image/png");
    let image_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png);
    let body = match build_http_request_body(config, &image_b64, mime_type, source, region, languages)
    {
        Ok(body) => body,
        Err(message) => return Err(CommandRunError::new(message, 0)),
    };
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms.max(1)))
        .build();
    let mut request = agent.post(url).set("Content-Type", "application/json");
    for (key, value) in headers {
        request = request.set(key, value);
    }
    let response = match request.send_json(body) {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(CommandRunError::new(
                format!("Region analysis HTTP provider failed with status {code}."),
                body.len(),
            ));
        }
        Err(ureq::Error::Transport(error)) => {
            let message = error.to_string();
            let timed_out = message.to_ascii_lowercase().contains("timed out")
                || message.to_ascii_lowercase().contains("timeout");
            return Err(CommandRunError::new(
                if timed_out {
                    format!(
                        "Region analysis HTTP provider timed out for page {page} region {region_id}."
                    )
                } else {
                    format!(
                        "Region analysis HTTP provider failed for page {page} region {region_id}."
                    )
                },
                0,
            ));
        }
    };
    let status = response.status();
    let stdout = response.into_string().map_err(|_| {
        CommandRunError::new(
            format!("Region analysis HTTP provider failed for page {page} region {region_id}."),
            0,
        )
    })?;
    if !(200..300).contains(&status) {
        return Err(CommandRunError::new(
            format!("Region analysis HTTP provider failed with status {status}."),
            stdout.len(),
        ));
    }
    parse_http_provider_stdout(preset.as_deref(), &stdout).map_err(|message| {
        CommandRunError::new(message, stdout.len())
    })
}

fn run_provider(
    config: &ProviderConfig,
    png: &[u8],
    source: &str,
    region: &Value,
    languages: &[String],
    timeout_ms: u64,
    max_output_chars: usize,
) -> Result<String, CommandRunError> {
    match config {
        ProviderConfig::Http { .. } => run_http_provider(config, png, source, region, languages, timeout_ms),
        ProviderConfig::Command {
            command,
            args_template,
        } => {
            let page = region["page"].as_u64().unwrap_or(0);
            let region_id = region["region_id"].as_str().unwrap_or("unknown");
            let temp = TempDir::with_prefix("pdf-reader-mcp-region-analysis-").map_err(|_| {
                CommandRunError::new(
                    "Failed to create region analysis provider workspace.".into(),
                    0,
                )
            })?;
            let input = temp.path().join(format!("region-{page}.png"));
            std::fs::write(&input, png).map_err(|_| {
                CommandRunError::new(
                    "Failed to write region analysis provider input image.".into(),
                    0,
                )
            })?;
            let input = input.to_string_lossy();
            let args = args_template
                .iter()
                .map(|arg| replace_placeholders(arg, &input, source, region, languages))
                .collect();
            command_provider::run(CommandInvocation {
                command: command.clone(),
                args,
                timeout_ms,
                max_stdout_bytes: max_output_chars.saturating_mul(4).max(1024 * 1024),
                failure_message: format!(
                    "Region analysis provider command failed for page {page} region {region_id}."
                ),
                timeout_message: format!(
                    "Region analysis provider command timed out for page {page} region {region_id}."
                ),
            })
        }
    }
}

fn text_result(payload: Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
    {
        let mut result = CallToolResult::structured(payload);
        result.content = vec![ContentBlock::text(text)];
        result
    }
}

fn error_result(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

/// True when a valid region-analysis provider is configured (command, HTTP, or preset).
pub(crate) fn command_provider_ready() -> bool {
    provider_config().is_ok()
}

/// Bound visual-enrichment analysis over already-admitted candidate regions.
///
/// Uses the shared crop/render/provider path and request-wide budgets. On the
/// first provider failure for a source, earlier analyses for that source are
/// discarded so public fusion matches TS fail-closed semantics.
pub(crate) fn analyze_visual_candidates_for_sources(
    sources: &[(usize, String, PathBuf, Vec<Value>)],
    max_regions: u32,
) -> Vec<anymd_core::SourceVisualOutcome> {
    use anymd_core::{SourceVisualOutcome, VISUAL_NO_CANDIDATE_WARNING};

    if sources.is_empty() {
        return Vec::new();
    }

    let config = match provider_config() {
        Ok(config) => config,
        Err(_) => {
            return sources
                .iter()
                .map(|(source_index, _, _, _)| SourceVisualOutcome {
                    source_index: *source_index,
                    provider_ready: false,
                    enrichments: Vec::new(),
                    warnings: Vec::new(),
                    error: None,
                })
                .collect();
        }
    };

    let max_regions = max_regions.clamp(1, 100);
    let timeout_ms = DEFAULT_TIMEOUT_MS;
    let max_output_chars = DEFAULT_MAX_OUTPUT_CHARS;
    let request_deadline = Instant::now() + Duration::from_millis(MAX_REQUEST_TIMEOUT_MS);
    let mut request_budget = RequestBudget::default();
    let mut outcomes = Vec::with_capacity(sources.len());

    // Shared concurrency permit for the whole request, matching analyze_regions.
    let _permit = match RequestPermit::acquire() {
        Ok(permit) => permit,
        Err(message) => {
            return sources
                .iter()
                .map(|(source_index, _, _, _)| SourceVisualOutcome {
                    source_index: *source_index,
                    provider_ready: true,
                    enrichments: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(message.clone()),
                })
                .collect();
        }
    };

    for (source_index, label, path, candidates) in sources {
        if candidates.is_empty() {
            outcomes.push(SourceVisualOutcome {
                source_index: *source_index,
                provider_ready: true,
                enrichments: Vec::new(),
                warnings: vec![VISUAL_NO_CANDIDATE_WARNING.into()],
                error: None,
            });
            continue;
        }

        let regions = candidates
            .iter()
            .filter_map(|candidate| {
                let region = candidate.get("region")?;
                let page = region.get("page").and_then(Value::as_u64)? as u32;
                let box_ = region.get("bounding_box")?;
                Some(json!({
                    "id": region.get("id").cloned().unwrap_or_else(|| candidate.get("id").cloned().unwrap_or(Value::Null)),
                    "page": page,
                    "bounding_box": {
                        "left": box_.get("left").cloned().unwrap_or(json!(0.0)),
                        "bottom": box_.get("bottom").cloned().unwrap_or(json!(0.0)),
                        "right": box_.get("right").cloned().unwrap_or(json!(0.0)),
                        "top": box_.get("top").cloned().unwrap_or(json!(0.0)),
                    }
                }))
            })
            .collect::<Vec<_>>();

        if regions.is_empty() {
            outcomes.push(SourceVisualOutcome {
                source_index: *source_index,
                provider_ready: true,
                enrichments: Vec::new(),
                warnings: vec![VISUAL_NO_CANDIDATE_WARNING.into()],
                error: None,
            });
            continue;
        }

        let crop_input = json!({
            "operation": "extract_regions",
            "include_image": true,
            "scale": 2.0,
            "max_regions": max_regions,
            "max_pixels_per_page": 16_000_000,
            "sources": [{
                "path": path.to_string_lossy(),
                "regions": regions,
            }]
        });

        let cropped = match visual_evidence::extract_regions(crop_input) {
            Ok(result) => result,
            Err(error) => {
                outcomes.push(SourceVisualOutcome {
                    source_index: *source_index,
                    provider_ready: true,
                    enrichments: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(error.message.to_string()),
                });
                continue;
            }
        };

        let encoded = match serde_json::to_value(&cropped) {
            Ok(value) => value,
            Err(error) => {
                outcomes.push(SourceVisualOutcome {
                    source_index: *source_index,
                    provider_ready: true,
                    enrichments: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(format!("Failed to encode crop evidence: {error}")),
                });
                continue;
            }
        };

        let Some(payload) = cropped.structured_content else {
            let message = encoded["content"][0]["text"]
                .as_str()
                .unwrap_or("All PDF sources failed region analysis.")
                .replace("failed region extraction", "failed region analysis");
            let message = message
                .strip_prefix("All PDF sources failed region analysis: ")
                .unwrap_or(&message)
                .to_string();
            outcomes.push(SourceVisualOutcome {
                source_index: *source_index,
                provider_ready: true,
                enrichments: Vec::new(),
                warnings: Vec::new(),
                error: Some(message),
            });
            continue;
        };

        let source = payload["results"]
            .as_array()
            .and_then(|results| results.first())
            .cloned()
            .unwrap_or_else(|| json!({"success": false, "error": "missing crop result"}));

        if source["success"] != true {
            let message = source["error"]
                .as_str()
                .unwrap_or("Region crop failed.")
                .to_string();
            outcomes.push(SourceVisualOutcome {
                source_index: *source_index,
                provider_ready: true,
                enrichments: Vec::new(),
                warnings: Vec::new(),
                error: Some(message),
            });
            continue;
        }

        let candidates_by_id = candidates
            .iter()
            .filter_map(|candidate| {
                let id = candidate
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| candidate.pointer("/region/id").and_then(Value::as_str))?;
                Some((id.to_string(), candidate))
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let analyzed = (|| -> Result<Vec<Value>, String> {
            let mut enrichments = Vec::new();
            for region in source["regions"].as_array().into_iter().flatten() {
                request_budget.ensure_available()?;
                let index = region["image_content_index"].as_u64().ok_or_else(|| {
                    "Cropped region analysis input image index is missing.".to_string()
                })?;
                let data = encoded["content"][index as usize]["data"]
                    .as_str()
                    .ok_or_else(|| "Cropped region analysis input image is missing.".to_string())?;
                let png = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data)
                    .map_err(|_| "Cropped region analysis input image is invalid.".to_string())?;
                let remaining_ms = u64::try_from(
                    request_deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                )
                .unwrap_or(u64::MAX);
                if remaining_ms == 0 {
                    return Err(format!(
                        "Request exceeds region analysis provider time limit of {MAX_REQUEST_TIMEOUT_MS} milliseconds."
                    ));
                }
                let stdout = match run_provider(
                    &config,
                    &png,
                    label,
                    region,
                    &[],
                    timeout_ms.min(remaining_ms),
                    max_output_chars,
                ) {
                    Ok(stdout) => stdout,
                    Err(error) => {
                        request_budget.charge_failed_provider(error.charge_bytes)?;
                        return Err(error.message);
                    }
                };
                let mut normalized = normalize_output(&stdout, max_output_chars);
                let output_chars = serde_json::to_string(&normalized)
                    .map_or(0, |value| value.encode_utf16().count());
                request_budget.charge(stdout.len(), output_chars)?;

                let region_id = region["region_id"].as_str().unwrap_or("").to_string();
                let candidate = candidates_by_id.get(&region_id).copied();
                let target_element_id = candidate
                    .and_then(|value| value.get("target_element_id"))
                    .and_then(Value::as_str)
                    .unwrap_or(region_id.as_str())
                    .to_string();
                let analysis_kind = normalized
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let target_element_type = candidate
                    .and_then(|value| value.get("target_element_type"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        if analysis_kind == "unknown" || analysis_kind == "text" {
                            "visual_region".into()
                        } else {
                            analysis_kind.into()
                        }
                    });

                // System-owned identity/provenance always win over provider payload.
                if let Some(object) = normalized.as_object_mut() {
                    object.insert("region_id".into(), json!(region_id));
                    object.insert("page".into(), region["page"].clone());
                    object.insert("provider".into(), json!(config.provider_label()));
                    object.insert(
                        "source_crop_evidence_id".into(),
                        region["evidence_id"].clone(),
                    );
                    object.insert(
                        "source_bounding_box".into(),
                        region["source_bounding_box"].clone(),
                    );
                    object.insert("crop_pixels".into(), region["crop_pixels"].clone());
                    object.insert("scale".into(), region["scale"].clone());
                    object.insert(
                        "provenance".into(),
                        json!({"engine": config.engine_label(), "source": "region-analysis-provider"}),
                    );
                    object.insert("id".into(), json!(format!("visual-{region_id}")));
                    object.insert("target_element_id".into(), json!(target_element_id));
                    object.insert("target_element_type".into(), json!(target_element_type));
                    if let Some(candidate) = candidate {
                        if let Some(value) = candidate.get("source_caption_element_id") {
                            object.insert("source_caption_element_id".into(), value.clone());
                        }
                        if let Some(value) = candidate.get("source_caption_text") {
                            object.insert("source_caption_text".into(), value.clone());
                        }
                        if let Some(value) = candidate.get("candidate_signals") {
                            object.insert("candidate_signals".into(), value.clone());
                        }
                        if let Some(value) = candidate.get("source_element_id") {
                            // Not part of public enrichment type, but never trust provider for it.
                            let _ = value;
                        }
                    }
                }
                enrichments.push(normalized);
            }
            Ok(enrichments)
        })();

        match analyzed {
            Ok(enrichments) => {
                let mut warnings = Vec::new();
                if let Some(crop_warnings) = source.get("warnings").and_then(Value::as_array) {
                    for warning in crop_warnings {
                        if let Some(message) = warning.as_str() {
                            warnings.push(message.to_string());
                        }
                    }
                }
                outcomes.push(SourceVisualOutcome {
                    source_index: *source_index,
                    provider_ready: true,
                    enrichments,
                    warnings,
                    error: None,
                });
            }
            Err(error) => {
                let message = error
                    .strip_prefix("All PDF sources failed region analysis: ")
                    .unwrap_or(&error)
                    .to_string();
                outcomes.push(SourceVisualOutcome {
                    source_index: *source_index,
                    provider_ready: true,
                    enrichments: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(message),
                });
            }
        }
    }

    outcomes
}

pub fn analyze_regions(value: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let args = parse_args(&value)?;
    if args.sources.len() > MAX_SOURCES_PER_REQUEST {
        return Err(rmcp::ErrorData::invalid_params(
            format!("pdf_evidence accepts at most {MAX_SOURCES_PER_REQUEST} sources per request."),
            None,
        ));
    }
    if args.sources.is_empty() {
        return Ok(error_result(
            "All PDF sources failed region analysis: ".into(),
        ));
    }
    let config = match provider_config() {
        Ok(config) => config,
        Err(message) => {
            return Ok(error_result(format!(
                "All PDF sources failed region analysis: {message}"
            )))
        }
    };
    let _permit = match RequestPermit::acquire() {
        Ok(permit) => permit,
        Err(message) => {
            return Ok(error_result(format!(
                "All PDF sources failed region analysis: {message}"
            )))
        }
    };
    let scale = args.scale.unwrap_or(2.0);
    let max_regions = args.max_regions.unwrap_or(20);
    let max_pixels = args.max_pixels_per_page.unwrap_or(16_000_000);
    let timeout_ms = u64::from(args.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32));
    let max_output_chars = args
        .max_output_chars
        .map_or(DEFAULT_MAX_OUTPUT_CHARS, |value| value as usize);
    let languages_provided = args.languages.is_some();
    let languages = args.languages.unwrap_or_default();
    let request_deadline = Instant::now() + Duration::from_millis(MAX_REQUEST_TIMEOUT_MS);
    let mut request_budget = RequestBudget::default();
    let mut crop_input = value;
    crop_input["operation"] = json!("extract_regions");
    crop_input["include_image"] = json!(true);
    let cropped = visual_evidence::extract_regions(crop_input)?;
    let encoded = serde_json::to_value(&cropped).map_err(|error| {
        rmcp::ErrorData::internal_error(format!("Failed to encode crop evidence: {error}"), None)
    })?;
    let Some(payload) = cropped.structured_content else {
        let message = encoded["content"][0]["text"]
            .as_str()
            .unwrap_or("All PDF sources failed region analysis.");
        return Ok(error_result(
            message.replace("failed region extraction", "failed region analysis"),
        ));
    };
    let mut results = Vec::new();
    for source in payload["results"].as_array().into_iter().flatten() {
        let source_label = source["source"].as_str().unwrap_or("unknown source");
        if source["success"] != true {
            results
                .push(json!({"source": source_label, "success": false, "error": source["error"]}));
            continue;
        }
        let output = (|| -> Result<Value, String> {
            let mut analyses = Vec::new();
            for region in source["regions"].as_array().into_iter().flatten() {
                request_budget.ensure_available()?;
                let index = region["image_content_index"].as_u64().ok_or_else(|| {
                    "Cropped region analysis input image index is missing.".to_string()
                })?;
                let data = encoded["content"][index as usize]["data"]
                    .as_str()
                    .ok_or_else(|| "Cropped region analysis input image is missing.".to_string())?;
                let png = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data)
                    .map_err(|_| "Cropped region analysis input image is invalid.".to_string())?;
                let remaining_ms = u64::try_from(
                    request_deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                )
                .unwrap_or(u64::MAX);
                if remaining_ms == 0 {
                    return Err(format!("Request exceeds region analysis provider time limit of {MAX_REQUEST_TIMEOUT_MS} milliseconds."));
                }
                let stdout = match run_provider(
                    &config,
                    &png,
                    source_label,
                    region,
                    &languages,
                    timeout_ms.min(remaining_ms),
                    max_output_chars,
                ) {
                    Ok(stdout) => stdout,
                    Err(error) => {
                        request_budget.charge_failed_provider(error.charge_bytes)?;
                        return Err(error.message);
                    }
                };
                let mut normalized = normalize_output(&stdout, max_output_chars);
                let output_chars = serde_json::to_string(&normalized)
                    .map_or(0, |value| value.encode_utf16().count());
                request_budget.charge(stdout.len(), output_chars)?;
                normalized["region_id"] = region["region_id"].clone();
                normalized["page"] = region["page"].clone();
                normalized["provider"] = json!(config.provider_label());
                normalized["source_crop_evidence_id"] = region["evidence_id"].clone();
                normalized["source_bounding_box"] = region["source_bounding_box"].clone();
                normalized["crop_pixels"] = region["crop_pixels"].clone();
                normalized["scale"] = region["scale"].clone();
                normalized["provenance"] =
                    json!({"engine": config.engine_label(), "source": "region-analysis-provider"});
                analyses.push(normalized);
            }
            let mut result = json!({"source": source_label, "success": true, "num_pages": source["num_pages"], "region_analyses": analyses});
            if source.get("warnings").is_some() {
                result["warnings"] = source["warnings"].clone();
            }
            Ok(result)
        })();
        results.push(output.unwrap_or_else(
            |error| json!({"source": source_label, "success": false, "error": error}),
        ));
    }
    if results.iter().all(|result| result["success"] != true) {
        let errors = results
            .iter()
            .filter_map(|result| result["error"].as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return Ok(error_result(format!(
            "All PDF sources failed region analysis: {errors}"
        )));
    }
    let mut options = json!({"scale": scale, "max_regions": max_regions, "max_pixels_per_page": max_pixels, "timeout_ms": timeout_ms, "max_output_chars": max_output_chars});
    if languages_provided {
        options["languages"] = json!(languages);
    }
    Ok(text_result(
        json!({"profile": "region_analysis", "analysis_options": options, "results": results}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_rich_aliases_and_unknown_kind_like_v3014() {
        let value = normalize_output(
            r#"{
          "kind":" SPREADSHEET ", "description":"  report  ", "confidence":87,
          "warnings":[" note ", 3],
          "table":{"rows":[["A",2,true,null]],"cells":[{"text":"x","row":1,"column":2,"rowspan":2,"colspan":3,"confidence":50,"bbox":{"left":1,"bottom":2,"right":3,"top":4}}]},
          "formula":{"ascii_math":"x^2","confidence":150},
          "chart":{"title":" T ","data_points":[{"x":1,"nested":{}}],"x_axis":{"label":" X ","min":0},"series":[{"name":"S","points":[{"y":2}],"confidence":90}]}
        }"#,
            200_000,
        );
        assert_eq!(value["kind"], "unknown");
        assert_eq!(value["description"], "report");
        assert_eq!(value["confidence"], 0.87);
        assert_eq!(value["table"]["row_count"], 1);
        assert_eq!(value["table"]["column_count"], 4);
        assert_eq!(value["table"]["cells"][0]["row_span"], 2);
        assert_eq!(value["formula"]["asciimath"], "x^2");
        assert_eq!(value["formula"]["confidence"], 1.0);
        assert_eq!(value["chart"]["series"][0]["confidence"], 0.9);
        assert_eq!(value["warnings"][0], "note");
        assert!(value["warnings"][1]
            .as_str()
            .unwrap()
            .contains("Unsupported"));
    }

    #[test]
    fn plain_text_truncates_by_utf16_units() {
        let value = normalize_output(" A😀B ", 3);
        assert_eq!(value["description"], "A😀");
        assert_eq!(
            value["warnings"][0],
            "Region analysis output truncated to 3 characters."
        );
    }

    #[test]
    fn accepts_http_and_preset_configuration_and_rejects_bad_command_args() {
        let http = provider_config_from(
            None,
            None,
            Some("http://127.0.0.1:9/analyze".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("http config");
        assert_eq!(http.provider_label(), "http");

        let ollama = provider_config_from(
            None,
            None,
            None,
            None,
            Some("ollama".into()),
            None,
            Some("llava".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("ollama config");
        assert_eq!(ollama.provider_label(), "http");

        assert!(provider_config_from(
            Some("provider".into()),
            Some(r#"["--flag"]"#.into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap_err()
        .contains("{input}"));

        assert!(provider_config_from(
            None,
            None,
            None,
            None,
            Some("nope".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap_err()
        .contains("Unsupported"));
    }

    #[test]
    fn http_provider_round_trip_against_local_mock() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            let body = br#"{"kind":"table","description":"mock http table","confidence":0.9}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                std::str::from_utf8(body).unwrap()
            );
            let _ = stream.write_all(response.as_bytes());
        });

        let config = provider_config_from(
            None,
            None,
            Some(format!("http://{addr}/analyze")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("http config");
        let region = json!({
            "page": 1,
            "region_id": "r1",
            "evidence_id": "e1",
            "mime_type": "image/png",
            "format": "png",
            "source_bounding_box": {"left": 0, "bottom": 0, "right": 10, "top": 10},
            "crop_pixels": {"width": 10, "height": 10},
            "scale": 2.0
        });
        let stdout = run_provider(
            &config,
            b"\x89PNG\r\n",
            "fixture.pdf",
            &region,
            &[],
            5_000,
            200_000,
        )
        .expect("http provider");
        assert!(stdout.contains("mock http table"));
        handle.join().expect("server thread");
    }

    #[test]
    fn request_budget_exhaustion_is_sticky() {
        let mut budget = RequestBudget::default();
        budget.charge(MAX_REQUEST_PROVIDER_BYTES - 1, 0).unwrap();
        assert!(budget.charge(2, 0).unwrap_err().contains("bytes"));
        assert!(budget.ensure_available().unwrap_err().contains("bytes"));
    }
}
