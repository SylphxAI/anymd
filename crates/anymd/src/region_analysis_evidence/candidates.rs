//! Request-wide orchestration for visual enrichment of admitted candidates.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::provider_budget::MAX_REQUEST_TIMEOUT_MS;

use super::candidate_regions::{
    attach_candidate_metadata, crop_candidate_regions, index_candidates, regions_for_crop,
};
use super::normalize::normalize_output;
use super::provider::run_provider;
use super::provider_config::provider_config;
use super::request_limits::{
    acquire_permit, refusal, timeout_refusal, RequestBudget, DEFAULT_MAX_OUTPUT_CHARS,
    DEFAULT_TIMEOUT_MS,
};

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
    let _permit = match acquire_permit() {
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

        let regions = regions_for_crop(candidates);

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

        let (source, encoded) = match crop_candidate_regions(crop_input) {
            Ok(crop) => crop,
            Err(message) => {
                outcomes.push(SourceVisualOutcome {
                    source_index: *source_index,
                    provider_ready: true,
                    enrichments: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(message),
                });
                continue;
            }
        };

        let candidates_by_id = index_candidates(candidates);

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
                    return Err(timeout_refusal());
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
                        request_budget.charge_failed_provider(error.charge_bytes, refusal)?;
                        return Err(error.message);
                    }
                };
                let mut normalized = normalize_output(&stdout, max_output_chars);
                let output_chars = serde_json::to_string(&normalized)
                    .map_or(0, |value| value.encode_utf16().count());
                request_budget.charge(stdout.len(), output_chars, refusal)?;

                attach_candidate_metadata(&mut normalized, region, &candidates_by_id, &config);
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
