use std::time::{Duration, Instant};

use anymd_core::{OcrPage, SourceOcrOutcome};
use serde_json::json;

use crate::provider_budget::MAX_REQUEST_TIMEOUT_MS;
use crate::visual_evidence::{self, MaterializedSource, OcrRenderRequest, RequestWorkBudget};

use super::normalize::{normalize_output, restore_render_geometry};
use super::provider::{provider_config, run_provider};
use super::request_limits::{
    acquire_permit, admit_read_ocr_source, refusal, RequestOcrBudget, MAX_SOURCES_PER_REQUEST,
};
use super::timeout_refusal;

mod options;
pub(crate) use options::ReadOcrOptions;

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
