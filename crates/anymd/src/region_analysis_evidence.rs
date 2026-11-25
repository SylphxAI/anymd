//! Optional command or HTTP provider analysis over bounded pure-Rust region crops.

use std::path::PathBuf;
use std::time::{Duration, Instant};

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

mod normalize;
mod provider;
mod provider_config;
#[cfg(test)]
mod tests;

use normalize::normalize_output;
use provider::run_provider;
use provider_config::{provider_config, ProviderConfig};

const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_MAX_OUTPUT_CHARS: usize = 200_000;
const MAX_SOURCES_PER_REQUEST: usize = 32;
/// A refusal in the region-analysis tool's words, given what the budget refused.
fn refusal(what: &str) -> String {
    match what {
        "provider output limit" => format!(
            "Request exceeds region analysis provider output limit of {MAX_REQUEST_PROVIDER_BYTES} bytes."
        ),
        "output limit" => format!(
            "Request exceeds region analysis output limit of {MAX_REQUEST_OUTPUT_CHARS} characters."
        ),
        other => format!("Request region analysis {other}."),
    }
}

/// What every region-analysis request refuses with once its time is up.
fn timeout_refusal() -> String {
    format!(
        "Request exceeds region analysis provider time limit of {MAX_REQUEST_TIMEOUT_MS} milliseconds."
    )
}

type RequestPermit = ProviderRequestPermit;
type RequestBudget = ProviderRequestBudget;

fn acquire_permit() -> Result<RequestPermit, String> {
    ProviderRequestPermit::acquire(|| {
        format!(
            "Region analysis provider concurrency limit of {MAX_CONCURRENT_PROVIDER_REQUESTS} active requests reached; retry later."
        )
    })
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
    let args = parse_pdf_evidence_args(&value)?;
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
    let _permit = match acquire_permit() {
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
                    return Err(timeout_refusal());
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
                        request_budget.charge_failed_provider(error.charge_bytes, refusal)?;
                        return Err(error.message);
                    }
                };
                let mut normalized = normalize_output(&stdout, max_output_chars);
                let output_chars = serde_json::to_string(&normalized)
                    .map_or(0, |value| value.encode_utf16().count());
                request_budget.charge(stdout.len(), output_chars, refusal)?;
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

/// Raster caption seam: only an explicitly configured local command is eligible.
pub(crate) fn local_caption_identity() -> Option<String> {
    let ProviderConfig::Command {
        command,
        args_template,
    } = provider_config().ok()?
    else {
        return None;
    };
    Some(format!("{command}:{args_template:?}"))
}

pub(crate) fn caption_raster(
    png: &[u8],
    source: &str,
    width: u32,
    height: u32,
    timeout_ms: u64,
) -> Result<crate::video_evidence::FrameDescription, String> {
    let config = provider_config()?;
    if !matches!(config, ProviderConfig::Command { .. }) {
        return Err("local-command caption provider required".into());
    }
    let region = json!({"region_id":"video-frame", "evidence_id":source, "source_bounding_box":{"left":0,"bottom":0,"right":width,"top":height}});
    let stdout = run_provider(&config, png, source, &region, &[], timeout_ms, 16_000)
        .map_err(|e| e.message)?;
    let value = normalize_output(&stdout, 16_000);
    Ok(crate::video_evidence::FrameDescription {
        text: value["description"].as_str().unwrap_or("").into(),
        provider: "external-command".into(),
        model_revision: serde_json::from_str::<Value>(&stdout)
            .ok()
            .and_then(|v| v["model_revision"].as_str().map(str::to_string)),
        truncated: value["truncated"].as_bool().unwrap_or(false)
            || value["warnings"].as_array().is_some_and(|warnings| {
                warnings
                    .iter()
                    .any(|v| v.as_str().is_some_and(|s| s.contains("truncated")))
            })
            || serde_json::from_str::<Value>(&stdout)
                .ok()
                .is_some_and(|v| v["truncated"].as_bool().unwrap_or(false)),
    })
}
