//! Optional command-provider OCR over the bounded pure-Rust page renderer.

use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anymd_core::{OcrPage, SourceOcrOutcome};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::command_provider::{self, CommandInvocation, CommandRunError};
use crate::schema::PdfEvidenceArgs;
use crate::visual_evidence;
use crate::visual_evidence::{MaterializedSource, OcrRenderRequest, RequestWorkBudget};

const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_MAX_OUTPUT_CHARS: usize = 200_000;
const OCR_COMMAND_ENV: &str = "MCP_PDF_OCR_COMMAND";
const OCR_ARGS_ENV: &str = "MCP_PDF_OCR_ARGS_JSON";
const OCR_PRESET_ENV: &str = "MCP_PDF_OCR_PRESET";
const MAX_SOURCES_PER_REQUEST: usize = 32;
const MAX_REQUEST_OCR_TIMEOUT_MS: u64 = 600_000;
const MAX_REQUEST_OCR_OUTPUT_CHARS: usize = 2_000_000;
const MAX_REQUEST_OCR_PROVIDER_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONCURRENT_OCR_REQUESTS: usize = 2;
static ACTIVE_OCR_REQUESTS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct OcrRequestPermit;

impl OcrRequestPermit {
    fn acquire() -> Result<Self, String> {
        ACTIVE_OCR_REQUESTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_CONCURRENT_OCR_REQUESTS).then_some(active + 1)
            })
            .map(|_| Self)
            .map_err(|_| {
                format!(
                    "OCR provider concurrency limit of {MAX_CONCURRENT_OCR_REQUESTS} active requests reached; retry later."
                )
            })
    }
}

impl Drop for OcrRequestPermit {
    fn drop(&mut self) {
        ACTIVE_OCR_REQUESTS.fetch_sub(1, Ordering::Release);
    }
}

#[derive(Default)]
struct RequestOcrBudget {
    output_chars: usize,
    provider_bytes: usize,
    exhausted: Option<String>,
}

impl RequestOcrBudget {
    fn ensure_available(&self) -> Result<(), String> {
        self.exhausted.clone().map_or(Ok(()), Err)
    }

    fn exhaust(&mut self, message: String) -> Result<(), String> {
        self.exhausted = Some(message.clone());
        Err(message)
    }

    fn charge(&mut self, provider_bytes: usize, output_chars: usize) -> Result<(), String> {
        self.ensure_available()?;
        let Some(next_provider_bytes) = self.provider_bytes.checked_add(provider_bytes) else {
            return self.exhaust("Request OCR provider byte count overflow.".to_string());
        };
        self.provider_bytes = next_provider_bytes;
        if self.provider_bytes > MAX_REQUEST_OCR_PROVIDER_BYTES {
            return self.exhaust(format!(
                "Request exceeds OCR provider output limit of {MAX_REQUEST_OCR_PROVIDER_BYTES} bytes."
            ));
        }
        let Some(next_output_chars) = self.output_chars.checked_add(output_chars) else {
            return self.exhaust("Request OCR output character count overflow.".to_string());
        };
        self.output_chars = next_output_chars;
        if self.output_chars > MAX_REQUEST_OCR_OUTPUT_CHARS {
            return self.exhaust(format!(
                "Request exceeds OCR output limit of {MAX_REQUEST_OCR_OUTPUT_CHARS} characters."
            ));
        }
        Ok(())
    }

    fn charge_failed_provider(&mut self, provider_bytes: usize) -> Result<(), String> {
        self.charge(provider_bytes, 0)
    }
}

fn admit_read_ocr_source(
    request_budget: &RequestOcrBudget,
    render_budget: &mut RequestWorkBudget,
    request_deadline: Instant,
) -> Result<(), String> {
    request_budget.ensure_available()?;
    render_budget.ensure_available()?;
    if Instant::now() >= request_deadline {
        return render_budget.exhaust(format!(
            "Request exceeds OCR provider time limit of {MAX_REQUEST_OCR_TIMEOUT_MS} milliseconds."
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OcrOutputFormat {
    Auto,
    TesseractTsv,
}

#[derive(Clone)]
struct ProviderConfig {
    command: String,
    args_template: Vec<String>,
    output_format: OcrOutputFormat,
}

fn parse_args(value: &Value) -> Result<PdfEvidenceArgs, rmcp::ErrorData> {
    let args: PdfEvidenceArgs = serde_json::from_value(value.clone()).map_err(|error| {
        rmcp::ErrorData::invalid_params(format!("Invalid pdf_evidence arguments: {error}"), None)
    })?;
    args.validate()
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    Ok(args)
}

fn provider_config_from(
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

fn provider_config() -> Result<ProviderConfig, String> {
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

fn run_provider(
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

fn normalize_confidence(value: &Value) -> Option<f64> {
    let value = value.as_f64()?;
    value.is_finite().then(|| {
        let value = if value > 1.0 { value / 100.0 } else { value };
        value.clamp(0.0, 1.0)
    })
}

fn round_coordinate(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn normalize_words(value: &Value, scale: f64) -> Option<Vec<Value>> {
    let words = value
        .as_array()?
        .iter()
        .filter_map(|word| {
            let word = word.as_object()?;
            let text = word.get("text")?.as_str()?;
            if text.trim().is_empty() {
                return None;
            }
            let mut output = json!({"text": text});
            if let Some(confidence) = word.get("confidence").and_then(normalize_confidence) {
                output["confidence"] = json!(confidence);
            }
            if let Some(box_) = word.get("bounding_box").and_then(Value::as_object) {
                let left = box_.get("left").and_then(Value::as_f64);
                let bottom = box_.get("bottom").and_then(Value::as_f64);
                let right = box_.get("right").and_then(Value::as_f64);
                let top = box_.get("top").and_then(Value::as_f64);
                if let (Some(left), Some(bottom), Some(right), Some(top)) =
                    (left, bottom, right, top)
                {
                    if [left, bottom, right, top]
                        .iter()
                        .all(|value| value.is_finite())
                        && right > left
                        && top > bottom
                    {
                        output["bounding_box"] = json!({
                            "left": round_coordinate(left / scale),
                            "bottom": round_coordinate(bottom / scale),
                            "right": round_coordinate(right / scale),
                            "top": round_coordinate(top / scale),
                        });
                    }
                }
            }
            Some(output)
        })
        .collect::<Vec<_>>();
    (!words.is_empty()).then_some(words)
}

fn truncate_utf16(text: &str, maximum: usize) -> (String, bool) {
    let mut units = 0usize;
    let mut end = text.len();
    for (index, character) in text.char_indices() {
        let next = units + character.len_utf16();
        if next > maximum {
            end = index;
            break;
        }
        units = next;
    }
    (
        text[..end].to_string(),
        text.encode_utf16().count() > maximum,
    )
}

fn parse_finite_number(value: Option<&str>) -> Option<f64> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let parsed = value.parse::<f64>().ok()?;
    parsed.is_finite().then_some(parsed)
}

fn required_tsv_column_indexes(headers: &[&str]) -> Option<[usize; 10]> {
    let index = |name: &str| headers.iter().position(|header| *header == name);
    let columns = [
        index("level")?,
        index("block_num")?,
        index("par_num")?,
        index("line_num")?,
        index("left")?,
        index("top")?,
        index("width")?,
        index("height")?,
        index("conf")?,
        index("text")?,
    ];
    Some(columns)
}

fn parse_tesseract_tsv_output(
    stdout: &str,
    max_output_chars: usize,
    languages: &[String],
    image_height: Option<f64>,
    scale: f64,
) -> Value {
    let lines: Vec<&str> = stdout.trim().lines().collect();
    let headers = lines
        .first()
        .map(|line| line.split('\t').collect::<Vec<_>>());
    let columns = headers
        .as_ref()
        .and_then(|headers| required_tsv_column_indexes(headers));
    let Some(columns) = columns else {
        let (text, truncated) = truncate_utf16(stdout.trim(), max_output_chars);
        let mut warnings = vec![
            "Tesseract TSV output could not be normalized; returned raw OCR output.".to_string(),
        ];
        if truncated {
            warnings.insert(
                0,
                format!("OCR output truncated to {max_output_chars} characters."),
            );
        }
        let mut output = json!({"text": text, "warnings": warnings});
        if let Some(language) = languages.first() {
            output["language"] = json!(language);
        }
        return output;
    };
    let Some(image_height) = image_height.filter(|height| *height > 0.0) else {
        let (text, truncated) = truncate_utf16(stdout.trim(), max_output_chars);
        let mut warnings = vec![
            "Tesseract TSV output could not be normalized; returned raw OCR output.".to_string(),
        ];
        if truncated {
            warnings.insert(
                0,
                format!("OCR output truncated to {max_output_chars} characters."),
            );
        }
        let mut output = json!({"text": text, "warnings": warnings});
        if let Some(language) = languages.first() {
            output["language"] = json!(language);
        }
        return output;
    };

    let [level_idx, block_idx, par_idx, line_idx, left_idx, top_idx, width_idx, height_idx, conf_idx, text_idx] =
        columns;

    let mut words = Vec::new();
    let mut line_texts: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for raw_line in lines.iter().skip(1) {
        if raw_line.trim().is_empty() {
            continue;
        }
        let values: Vec<&str> = raw_line.split('\t').collect();
        let level = parse_finite_number(values.get(level_idx).copied());
        let text = values
            .get(text_idx..)
            .map(|parts| parts.join("	").trim().to_string())
            .unwrap_or_default();
        if level != Some(5.0) || text.is_empty() {
            continue;
        }
        let left = parse_finite_number(values.get(left_idx).copied());
        let top = parse_finite_number(values.get(top_idx).copied());
        let width = parse_finite_number(values.get(width_idx).copied());
        let height = parse_finite_number(values.get(height_idx).copied());
        let confidence = parse_finite_number(values.get(conf_idx).copied())
            .and_then(|value| normalize_confidence(&json!(value)));
        let line_key = format!(
            "{}:{}:{}",
            values.get(block_idx).copied().unwrap_or("0"),
            values.get(par_idx).copied().unwrap_or("0"),
            values.get(line_idx).copied().unwrap_or("0")
        );
        line_texts.entry(line_key).or_default().push(text.clone());

        let mut word = json!({"text": text});
        if let Some(confidence) = confidence {
            word["confidence"] = json!(confidence);
        }
        if let (Some(left), Some(top), Some(width), Some(height)) = (left, top, width, height) {
            if width > 0.0 && height > 0.0 {
                // image-space bottom-left box; convert to PDF coords via scale below
                let image_box = json!({
                    "left": left,
                    "bottom": image_height - top - height,
                    "right": left + width,
                    "top": image_height - top,
                });
                word["bounding_box"] = image_box;
            }
        }
        words.push(word);
    }

    let raw_text = line_texts
        .values()
        .map(|line| line.join(" "))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let (text, truncated) = truncate_utf16(&raw_text, max_output_chars);
    let mut output = json!({"text": text});
    let confidences = words
        .iter()
        .filter_map(|word| word.get("confidence").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    if !confidences.is_empty() {
        let average = confidences.iter().sum::<f64>() / confidences.len() as f64;
        output["confidence"] = json!((average * 100.0).round() / 100.0);
    }
    if !words.is_empty() {
        // wrap through normalize_words to apply scale conversion
        let words_value = Value::Array(words);
        if let Some(normalized) = normalize_words(&words_value, scale) {
            output["words"] = json!(normalized);
        }
    }
    if let Some(language) = languages.first() {
        output["language"] = json!(language);
    }
    if truncated {
        output["warnings"] = json!([format!(
            "OCR output truncated to {max_output_chars} characters."
        )]);
    }
    output
}

fn normalize_output(
    stdout: &str,
    max_output_chars: usize,
    languages: &[String],
    scale: f64,
    output_format: OcrOutputFormat,
    image_height: Option<f64>,
) -> Value {
    if output_format == OcrOutputFormat::TesseractTsv {
        return parse_tesseract_tsv_output(
            stdout,
            max_output_chars,
            languages,
            image_height,
            scale,
        );
    }
    let trimmed = stdout.trim();
    let parsed = serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object);
    let raw_text = parsed
        .as_ref()
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
        .unwrap_or(trimmed);
    let (text, truncated) = truncate_utf16(raw_text, max_output_chars);
    let mut output = json!({"text": text});
    if let Some(confidence) = parsed
        .as_ref()
        .and_then(|value| value.get("confidence"))
        .and_then(normalize_confidence)
    {
        output["confidence"] = json!(confidence);
    }
    if let Some(words) = parsed
        .as_ref()
        .and_then(|value| value.get("words"))
        .and_then(|value| normalize_words(value, scale))
    {
        output["words"] = json!(words);
    }
    if let Some(language) = parsed
        .as_ref()
        .and_then(|value| value.get("language"))
        .and_then(Value::as_str)
        .or_else(|| languages.first().map(String::as_str))
    {
        output["language"] = json!(language);
    }
    if truncated {
        output["warnings"] = json!([format!(
            "OCR output truncated to {max_output_chars} characters."
        )]);
    }
    output
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReadOcrOptions {
    pub(crate) scale: f64,
    pub(crate) max_pages: usize,
    pub(crate) max_pixels_per_page: u64,
    pub(crate) timeout_ms: u64,
    pub(crate) max_output_chars: usize,
}

impl Default for ReadOcrOptions {
    fn default() -> Self {
        Self {
            scale: 2.0,
            max_pages: 5,
            max_pixels_per_page: 16_000_000,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_output_chars: DEFAULT_MAX_OUTPUT_CHARS,
        }
    }
}

/// Render and OCR read_pdf candidates without converting image bytes through
/// MCP content/base64. All effects are synchronous by design; callers must run
/// this boundary on a blocking worker.
pub(crate) fn run_read_ocr(
    sources: &[(&MaterializedSource, Vec<u32>)],
    options: ReadOcrOptions,
) -> Vec<SourceOcrOutcome> {
    if sources.len() > MAX_SOURCES_PER_REQUEST {
        let error =
            format!("read_pdf OCR accepts at most {MAX_SOURCES_PER_REQUEST} sources per request.");
        return sources
            .iter()
            .map(|(source, _)| SourceOcrOutcome {
                source_index: source.source_index(),
                pages: Vec::new(),
                warnings: Vec::new(),
                error: Some(error.clone()),
            })
            .collect();
    }
    let config = match provider_config() {
        Ok(config) => config,
        Err(error) => {
            return sources
                .iter()
                .map(|(source, _)| SourceOcrOutcome {
                    source_index: source.source_index(),
                    pages: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(error.clone()),
                })
                .collect()
        }
    };
    let _permit = match OcrRequestPermit::acquire() {
        Ok(permit) => permit,
        Err(error) => {
            return sources
                .iter()
                .map(|(source, _)| SourceOcrOutcome {
                    source_index: source.source_index(),
                    pages: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(error.clone()),
                })
                .collect()
        }
    };

    let request_deadline = Instant::now() + Duration::from_millis(MAX_REQUEST_OCR_TIMEOUT_MS);
    let mut request_budget = RequestOcrBudget::default();
    let mut render_budget = RequestWorkBudget::default();
    let languages = Vec::new();
    sources
        .iter()
        .map(|(source, candidate_pages)| {
            let deadline_error = format!(
                "Request exceeds OCR provider time limit of {MAX_REQUEST_OCR_TIMEOUT_MS} milliseconds."
            );
            if let Err(error) =
                admit_read_ocr_source(&request_budget, &mut render_budget, request_deadline)
            {
                return SourceOcrOutcome {
                    source_index: source.source_index(),
                    pages: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(error),
                };
            }
            let rendered = visual_evidence::render_ocr_source(
                source,
                OcrRenderRequest {
                    requested_pages: candidate_pages,
                    scale_value: options.scale,
                    max_pages: options.max_pages,
                    max_pixels: options.max_pixels_per_page,
                    request_deadline,
                    deadline_error: &deadline_error,
                },
                &mut render_budget,
            );
            if let Some(error) = rendered.error {
                return SourceOcrOutcome {
                    source_index: rendered.source_index,
                    pages: Vec::new(),
                    warnings: rendered.warnings,
                    error: Some(error),
                };
            }
            let mut pages = Vec::with_capacity(rendered.pages.len());
            for page in rendered.pages {
                if let Err(error) = request_budget.ensure_available() {
                    return SourceOcrOutcome {
                        source_index: rendered.source_index,
                        pages: Vec::new(),
                        warnings: rendered.warnings,
                        error: Some(error),
                    };
                }
                let remaining_ms = u64::try_from(
                    request_deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                )
                .unwrap_or(u64::MAX);
                if remaining_ms == 0 {
                    return SourceOcrOutcome {
                        source_index: rendered.source_index,
                        pages: Vec::new(),
                        warnings: rendered.warnings,
                        error: Some(format!(
                            "Request exceeds OCR provider time limit of {MAX_REQUEST_OCR_TIMEOUT_MS} milliseconds."
                        )),
                    };
                }
                let stdout = match run_provider(
                    &config,
                    &page.png,
                    u64::from(page.page),
                    &rendered.source,
                    &languages,
                    options.timeout_ms.min(remaining_ms),
                    options.max_output_chars,
                ) {
                    Ok(stdout) => stdout,
                    Err(error) => {
                        let _ = request_budget.charge_failed_provider(error.charge_bytes);
                        return SourceOcrOutcome {
                            source_index: rendered.source_index,
                            pages: Vec::new(),
                            warnings: rendered.warnings,
                            error: Some(error.message),
                        };
                    }
                };
                let mut normalized = normalize_output(
                    &stdout,
                    options.max_output_chars,
                    &languages,
                    options.scale,
                    config.output_format,
                    Some(f64::from(page.height)),
                );
                let output_chars = normalized["text"]
                    .as_str()
                    .map_or(0, |text| text.encode_utf16().count());
                if let Err(error) = request_budget.charge(stdout.len(), output_chars) {
                    return SourceOcrOutcome {
                        source_index: rendered.source_index,
                        pages: Vec::new(),
                        warnings: rendered.warnings,
                        error: Some(error),
                    };
                }
                normalized["page"] = json!(page.page);
                normalized["provider"] = json!("command");
                normalized["source_render_evidence_id"] = json!(page.evidence_id);
                normalized["source_render_scale"] = json!(page.scale);
                normalized["source_render_width"] = json!(page.width);
                normalized["source_render_height"] = json!(page.height);
                normalized["provenance"] =
                    json!({"engine": "external-command", "source": "ocr-provider"});
                match serde_json::from_value::<OcrPage>(normalized) {
                    Ok(page) => pages.push(page),
                    Err(error) => {
                        return SourceOcrOutcome {
                            source_index: rendered.source_index,
                            pages: Vec::new(),
                            warnings: rendered.warnings,
                            error: Some(format!("Failed to normalize OCR provider output: {error}")),
                        }
                    }
                }
            }
            SourceOcrOutcome {
                source_index: rendered.source_index,
                pages,
                warnings: rendered.warnings,
                error: None,
            }
        })
        .collect()
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

pub fn ocr_pages(value: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let args = parse_args(&value)?;
    if args.sources.len() > MAX_SOURCES_PER_REQUEST {
        return Err(rmcp::ErrorData::invalid_params(
            format!("pdf_evidence accepts at most {MAX_SOURCES_PER_REQUEST} sources per request."),
            None,
        ));
    }
    if args.sources.is_empty() {
        return Ok(error_result("All PDF sources failed OCR: ".into()));
    }
    let config = match provider_config() {
        Ok(config) => config,
        Err(message) => {
            return Ok(error_result(format!(
                "All PDF sources failed OCR: {message}"
            )))
        }
    };
    let _permit = match OcrRequestPermit::acquire() {
        Ok(permit) => permit,
        Err(message) => {
            return Ok(error_result(format!(
                "All PDF sources failed OCR: {message}"
            )))
        }
    };
    let scale = args.scale.unwrap_or(2.0);
    let max_pages = args.max_pages.unwrap_or(5);
    let max_pixels = args.max_pixels_per_page.unwrap_or(16_000_000);
    let timeout_ms = u64::from(args.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32));
    let max_output_chars = args
        .max_output_chars
        .map_or(DEFAULT_MAX_OUTPUT_CHARS, |value| value as usize);
    let languages_provided = args.languages.is_some();
    let languages = args.languages.unwrap_or_default();
    let request_deadline = Instant::now() + Duration::from_millis(MAX_REQUEST_OCR_TIMEOUT_MS);
    let mut request_budget = RequestOcrBudget::default();
    let mut render_input = value;
    render_input["operation"] = json!("render_page");
    render_input["include_image"] = json!(true);
    let rendered = visual_evidence::render_pages(render_input)?;
    let encoded = serde_json::to_value(&rendered).map_err(|error| {
        rmcp::ErrorData::internal_error(format!("Failed to encode render evidence: {error}"), None)
    })?;
    let Some(payload) = rendered.structured_content else {
        let message = encoded["content"][0]["text"]
            .as_str()
            .unwrap_or("All PDF sources failed OCR.");
        return Ok(error_result(
            message.replace("failed to render", "failed OCR"),
        ));
    };
    let mut results = Vec::new();
    for source in payload["results"].as_array().into_iter().flatten() {
        let source_label = source["source"].as_str().unwrap_or("unknown source");
        if source["success"] != true {
            results.push(json!({
                "source": source_label,
                "success": false,
                "error": source["error"],
            }));
            continue;
        }
        let output = (|| -> Result<Value, String> {
            let mut ocr_pages = Vec::new();
            for page in source["rendered_pages"].as_array().into_iter().flatten() {
                request_budget.ensure_available()?;
                let page_number = page["page"].as_u64().unwrap_or(0);
                let index = page["image_content_index"]
                    .as_u64()
                    .ok_or_else(|| "Rendered OCR input image index is missing.".to_string())?;
                let data = encoded["content"][index as usize]["data"]
                    .as_str()
                    .ok_or_else(|| "Rendered OCR input image is missing.".to_string())?;
                let png = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data)
                    .map_err(|_| "Rendered OCR input image is invalid.".to_string())?;
                let remaining_ms = u64::try_from(
                    request_deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                )
                .unwrap_or(u64::MAX);
                if remaining_ms == 0 {
                    return Err(format!(
                        "Request exceeds OCR provider time limit of {MAX_REQUEST_OCR_TIMEOUT_MS} milliseconds."
                    ));
                }
                let stdout = match run_provider(
                    &config,
                    &png,
                    page_number,
                    source_label,
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
                let mut normalized = normalize_output(
                    &stdout,
                    max_output_chars,
                    &languages,
                    scale,
                    config.output_format,
                    page.get("height").and_then(Value::as_f64),
                );
                let output_chars = normalized["text"]
                    .as_str()
                    .map_or(0, |text| text.encode_utf16().count());
                request_budget.charge(stdout.len(), output_chars)?;
                normalized["page"] = json!(page_number);
                normalized["provider"] = json!("command");
                normalized["source_render_evidence_id"] = page["evidence_id"].clone();
                normalized["source_render_scale"] = page["scale"].clone();
                normalized["source_render_width"] = page["width"].clone();
                normalized["source_render_height"] = page["height"].clone();
                normalized["provenance"] =
                    json!({"engine": "external-command", "source": "ocr-provider"});
                ocr_pages.push(normalized);
            }
            let mut result = json!({
                "source": source_label,
                "success": true,
                "num_pages": source["num_pages"],
                "ocr_pages": ocr_pages,
            });
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
            "All PDF sources failed OCR: {errors}"
        )));
    }
    let mut options = json!({
        "scale": scale,
        "max_pages": max_pages,
        "max_pixels_per_page": max_pixels,
        "timeout_ms": timeout_ms,
        "max_output_chars": max_output_chars,
    });
    if languages_provided {
        options["languages"] = json!(languages);
    }
    Ok(text_result(json!({
        "profile": "ocr_text_layer",
        "ocr_options": options,
        "results": results,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_json_provider_output_and_utf16_truncation() {
        let output = normalize_output(
            r#"{"text":"A😀B","confidence":87,"language":"fra","words":[{"text":"A","confidence":50,"bounding_box":{"left":2,"bottom":4,"right":6,"top":8}}]}"#,
            3,
            &[],
            2.0,
            OcrOutputFormat::Auto,
            None,
        );
        assert_eq!(output["text"], "A😀");
        assert_eq!(output["confidence"], 0.87);
        assert_eq!(output["language"], "fra");
        assert_eq!(output["words"][0]["confidence"], 0.5);
        assert_eq!(
            output["words"][0]["bounding_box"],
            json!({"left": 1.0, "bottom": 2.0, "right": 3.0, "top": 4.0})
        );
        assert_eq!(
            output["warnings"][0],
            "OCR output truncated to 3 characters."
        );
    }

    #[test]
    fn rejects_invalid_provider_configuration_without_shell_parsing() {
        let error = provider_config_from(
            Some("provider".into()),
            None,
            Some(r#"["--no-input"]"#.into()),
        )
        .err()
        .expect("invalid args");
        assert!(error.contains("{input}"));
        let tsv = provider_config_from(None, Some("tesseract-tsv".into()), None)
            .expect("tsv preset is available");
        assert_eq!(tsv.command, "tesseract");
        assert_eq!(tsv.output_format, OcrOutputFormat::TesseractTsv);
        assert!(tsv.args_template.iter().any(|arg| arg == "tsv"));
    }

    #[test]
    fn normalizes_tesseract_tsv_words_and_image_to_pdf_boxes() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t20\t10\t40\t20\t91\tHello\n5\t1\t1\t1\t1\t2\t70\t10\t30\t20\t88\tWorld\n";
        let output = normalize_output(
            tsv,
            200_000,
            &["eng".into()],
            2.0,
            OcrOutputFormat::TesseractTsv,
            Some(100.0),
        );
        assert_eq!(output["text"], "Hello World");
        assert_eq!(output["language"], "eng");
        assert_eq!(output["words"][0]["text"], "Hello");
        // image box left=20,top=10,width=40,height=20,imageHeight=100
        // bottom = 100-10-20=70, top=90; then /scale=2 => left=10,bottom=35,right=30,top=45
        assert_eq!(
            output["words"][0]["bounding_box"],
            json!({"left": 10.0, "bottom": 35.0, "right": 30.0, "top": 45.0})
        );
        assert_eq!(output["words"][0]["confidence"], 0.91);
    }

    #[test]
    fn malformed_tesseract_tsv_returns_raw_with_warning() {
        let output = normalize_output(
            "not\ta\theader\nrow",
            200_000,
            &[],
            2.0,
            OcrOutputFormat::TesseractTsv,
            Some(100.0),
        );
        assert_eq!(output["text"], "not\ta\theader\nrow");
        assert_eq!(
            output["warnings"][0],
            "Tesseract TSV output could not be normalized; returned raw OCR output."
        );
        assert!(output.get("words").is_none());
    }

    #[test]
    fn request_budget_is_request_wide_and_fail_closed() {
        let mut budget = RequestOcrBudget::default();
        budget
            .charge(
                MAX_REQUEST_OCR_PROVIDER_BYTES - 1,
                MAX_REQUEST_OCR_OUTPUT_CHARS - 1,
            )
            .expect("within budget");
        assert!(budget
            .charge(2, 0)
            .expect_err("byte overflow")
            .contains("bytes"));

        let mut budget = RequestOcrBudget::default();
        budget
            .charge(0, MAX_REQUEST_OCR_OUTPUT_CHARS - 1)
            .expect("within budget");
        assert!(budget
            .charge(0, 2)
            .expect_err("character overflow")
            .contains("characters"));
        assert!(budget
            .ensure_available()
            .expect_err("exhaustion is sticky")
            .contains("characters"));
    }

    #[test]
    fn exhausted_provider_budget_blocks_later_render_and_provider_effects() {
        let mut request_budget = RequestOcrBudget::default();
        request_budget
            .charge(0, MAX_REQUEST_OCR_OUTPUT_CHARS + 1)
            .expect_err("exhaust provider budget");
        let mut render_budget = RequestWorkBudget::default();
        let mut sentinel_invocations = 0usize;

        if admit_read_ocr_source(
            &request_budget,
            &mut render_budget,
            Instant::now() + Duration::from_secs(1),
        )
        .is_ok()
        {
            sentinel_invocations += 1;
        }

        assert_eq!(sentinel_invocations, 0);
        assert_eq!(render_budget.rendered_pages, 0);
        assert_eq!(render_budget.render_pixels, 0);
    }

    #[test]
    fn provider_concurrency_admission_is_process_wide_and_recoverable() {
        let first = OcrRequestPermit::acquire().expect("first permit");
        let second = OcrRequestPermit::acquire().expect("second permit");
        assert!(OcrRequestPermit::acquire()
            .expect_err("third request rejected")
            .contains("concurrency limit"));
        drop(first);
        let replacement = OcrRequestPermit::acquire().expect("released slot is reusable");
        drop(second);
        drop(replacement);
        assert_eq!(ACTIVE_OCR_REQUESTS.load(Ordering::Acquire), 0);
    }
}
