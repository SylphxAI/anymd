//! Optional command-provider OCR over the bounded pure-Rust page renderer.

use std::time::{Duration, Instant};

use anymd_core::{OcrPage, SourceOcrOutcome};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::pdf_evidence_common::{
    error_result, json_result as text_result, parse_pdf_evidence_args,
};
use crate::provider_budget::{
    ProviderRequestBudget, ProviderRequestPermit, MAX_CONCURRENT_PROVIDER_REQUESTS,
    MAX_REQUEST_OUTPUT_CHARS, MAX_REQUEST_PROVIDER_BYTES, MAX_REQUEST_TIMEOUT_MS,
};
use crate::visual_evidence;
use crate::visual_evidence::{MaterializedSource, OcrRenderRequest, RequestWorkBudget};

mod normalize;
mod provider;
#[cfg(test)]
mod tests;

use normalize::{normalize_output, raster_observation, restore_render_geometry};
use provider::{provider_config, run_provider};

const DEFAULT_MAX_OUTPUT_CHARS: usize = 200_000;
const MAX_SOURCES_PER_REQUEST: usize = 32;
/// A refusal in the OCR tool's words, given what the budget refused.
fn refusal(what: &str) -> String {
    match what {
        "provider output limit" => format!(
            "Request exceeds OCR provider output limit of {MAX_REQUEST_PROVIDER_BYTES} bytes."
        ),
        "output limit" => {
            format!("Request exceeds OCR output limit of {MAX_REQUEST_OUTPUT_CHARS} characters.")
        }
        other => format!("Request {other}."),
    }
}

/// What every OCR request refuses with once its time is up.
pub(crate) fn timeout_refusal() -> String {
    format!("Request exceeds OCR provider time limit of {MAX_REQUEST_TIMEOUT_MS} milliseconds.")
}

type RequestOcrBudget = ProviderRequestBudget;
/// The OCR tool's concurrency slot: one shared process-wide provider budget.
pub(crate) type OcrRequestPermit = ProviderRequestPermit;

/// The one admission call every OCR-path caller uses.
pub(crate) fn acquire_ocr_permit() -> Result<OcrRequestPermit, String> {
    acquire_permit()
}

fn acquire_permit() -> Result<OcrRequestPermit, String> {
    ProviderRequestPermit::acquire(|| {
        format!(
            "OCR provider concurrency limit of {MAX_CONCURRENT_PROVIDER_REQUESTS} active requests reached; retry later."
        )
    })
}

fn admit_read_ocr_source(
    request_budget: &RequestOcrBudget,
    render_budget: &mut RequestWorkBudget,
    request_deadline: Instant,
) -> Result<(), String> {
    request_budget.ensure_available()?;
    render_budget.ensure_available()?;
    if Instant::now() >= request_deadline {
        return render_budget.exhaust(timeout_refusal());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReadOcrOptions {
    pub(crate) engine: Option<crate::ocr_vlm::OcrEngine>,
    pub(crate) deadline: Option<Instant>,
    pub(crate) exact_geometry: bool,
    pub(crate) scale: f64,
    pub(crate) max_pages: usize,
    pub(crate) max_pixels_per_page: u64,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) max_output_chars: usize,
}

impl Default for ReadOcrOptions {
    fn default() -> Self {
        Self {
            engine: None,
            deadline: None,
            exact_geometry: false,
            scale: 2.0,
            max_pages: 5,
            max_pixels_per_page: 16_000_000,
            timeout_ms: None,
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
    let config = match provider_config(options.engine) {
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
    let timeout_ms = match config.page_timeout(options.timeout_ms) {
        Ok(timeout) => timeout,
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
    let _permit = match acquire_permit() {
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

    let request_deadline = options
        .deadline
        .unwrap_or_else(|| Instant::now() + Duration::from_millis(MAX_REQUEST_TIMEOUT_MS));
    let mut request_budget = RequestOcrBudget::default();
    let mut render_budget = RequestWorkBudget::default();
    let languages = Vec::new();
    sources
        .iter()
        .map(|(source, candidate_pages)| {
            let deadline_error = timeout_refusal();
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
                        pages: if options.exact_geometry {
                            pages
                        } else {
                            Vec::new()
                        },
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
                        pages: if options.exact_geometry {
                            pages
                        } else {
                            Vec::new()
                        },
                        warnings: rendered.warnings,
                        error: Some(timeout_refusal()),
                    };
                }
                let stdout = match run_provider(
                    &config,
                    &page.png,
                    u64::from(page.page),
                    &rendered.source,
                    &languages,
                    timeout_ms.min(remaining_ms),
                    options.max_output_chars,
                ) {
                    Ok(stdout) => stdout,
                    Err(error) => {
                        let _ = request_budget.charge_failed_provider(error.charge_bytes, refusal);
                        return SourceOcrOutcome {
                            source_index: rendered.source_index,
                            pages: if options.exact_geometry {
                                pages
                            } else {
                                Vec::new()
                            },
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
                if options.exact_geometry {
                    restore_render_geometry(
                        &mut normalized,
                        page.pdf_to_pixel,
                        page.scale,
                        page.height,
                    );
                }
                let output_chars = normalized["text"]
                    .as_str()
                    .map_or(0, |text| text.encode_utf16().count());
                if let Err(error) = request_budget.charge(stdout.len(), output_chars, refusal) {
                    return SourceOcrOutcome {
                        source_index: rendered.source_index,
                        pages: if options.exact_geometry {
                            pages
                        } else {
                            Vec::new()
                        },
                        warnings: rendered.warnings,
                        error: Some(error),
                    };
                }
                normalized["page"] = json!(page.page);
                normalized["provider"] = json!(if config.builtin_vlm() {
                    "candle"
                } else {
                    "command"
                });
                normalized["source_render_evidence_id"] = json!(page.evidence_id);
                normalized["source_render_scale"] = json!(page.scale);
                normalized["source_render_width"] = json!(page.width);
                normalized["source_render_height"] = json!(page.height);
                config.stamp_provenance(&mut normalized);
                match serde_json::from_value::<OcrPage>(normalized) {
                    Ok(page) => pages.push(page),
                    Err(error) => {
                        return SourceOcrOutcome {
                            source_index: rendered.source_index,
                            pages: if options.exact_geometry {
                                pages
                            } else {
                                Vec::new()
                            },
                            warnings: rendered.warnings,
                            error: Some(format!(
                                "Failed to normalize OCR provider output: {error}"
                            )),
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

pub fn ocr_pages(value: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let args = parse_pdf_evidence_args(&value)?;
    if args.sources.len() > MAX_SOURCES_PER_REQUEST {
        return Err(rmcp::ErrorData::invalid_params(
            format!("pdf_evidence accepts at most {MAX_SOURCES_PER_REQUEST} sources per request."),
            None,
        ));
    }
    if args.sources.is_empty() {
        return Ok(error_result("All PDF sources failed OCR: ".into()));
    }
    let config = match provider_config(args.ocr) {
        Ok(config) => config,
        Err(message) => {
            return Ok(error_result(format!(
                "All PDF sources failed OCR: {message}"
            )))
        }
    };
    let _permit = match acquire_permit() {
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
    let timeout_ms = config
        .page_timeout(args.timeout_ms.map(u64::from))
        .map_err(|e| rmcp::ErrorData::invalid_params(e, None))?;
    let max_output_chars = args
        .max_output_chars
        .map_or(DEFAULT_MAX_OUTPUT_CHARS, |value| value as usize);
    let languages_provided = args.languages.is_some();
    let languages = args.languages.unwrap_or_default();
    let request_deadline = Instant::now() + Duration::from_millis(MAX_REQUEST_TIMEOUT_MS);
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
                    return Err(timeout_refusal());
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
                        request_budget.charge_failed_provider(error.charge_bytes, refusal)?;
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
                request_budget.charge(stdout.len(), output_chars, refusal)?;
                normalized["page"] = json!(page_number);
                normalized["provider"] = json!(if config.builtin_vlm() {
                    "candle"
                } else {
                    "command"
                });
                normalized["source_render_evidence_id"] = page["evidence_id"].clone();
                normalized["source_render_scale"] = page["scale"].clone();
                normalized["source_render_width"] = page["width"].clone();
                normalized["source_render_height"] = page["height"].clone();
                config.stamp_provenance(&mut normalized);
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

/// Video raster input borrows the same OCR request permit; no PDF-space fiction.
pub(crate) fn recognize_raster(
    frame: &anymd_formats::video::timeline::DecodedFrame,
    engine: crate::ocr_vlm::OcrEngine,
    deadline: Instant,
    permit: &OcrRequestPermit,
) -> Result<crate::video_evidence::RasterObservation, String> {
    let config = provider_config(Some(engine))?;
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64;
    if remaining == 0 {
        return Err("video request deadline exceeded".into());
    }
    let mut value = if config.builtin_vlm() {
        let page = crate::ocr_vlm::recognize_admitted(&frame.png, deadline, permit)?;
        let mut value = serde_json::to_value(&page).map_err(|e| e.to_string())?;
        value["words"] = json!(page.regions.iter().map(|r| json!({"text":r.text,"region_type":r.label,"bounding_box":{"left":r.bbox[0],"right":r.bbox[2],"top":frame.metadata.height as f32-r.bbox[1],"bottom":frame.metadata.height as f32-r.bbox[3]}})).collect::<Vec<_>>());
        value["model_revision"] = json!(crate::ocr_vlm::model_revision());
        value
    } else {
        let stdout = run_provider(
            &config,
            &frame.png,
            0,
            &frame.metadata.sha256,
            &[],
            remaining.min(60_000),
            200_000,
        )
        .map_err(|e| e.message)?;
        normalize_output(
            &stdout,
            200_000,
            &[],
            1.0,
            config.output_format,
            Some(frame.metadata.height as f64),
        )
    };
    config.stamp_provenance(&mut value);
    raster_observation(
        &value,
        f64::from(frame.metadata.height),
        config.builtin_vlm(),
    )
}
