//! MCP region-analysis request handling.

use std::time::{Duration, Instant};

use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::pdf_evidence_common::{
    error_result, json_result as text_result, parse_pdf_evidence_args,
};
use crate::provider_budget::MAX_REQUEST_TIMEOUT_MS;
use crate::visual_evidence;

use super::normalize::normalize_output;
use super::provider::run_provider;
use super::provider_config::provider_config;
use super::request_limits::{
    acquire_permit, refusal, timeout_refusal, RequestBudget, DEFAULT_MAX_OUTPUT_CHARS,
    DEFAULT_TIMEOUT_MS,
};

const MAX_SOURCES_PER_REQUEST: usize = 32;

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
