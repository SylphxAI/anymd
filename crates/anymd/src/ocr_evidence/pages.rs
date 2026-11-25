use std::time::{Duration, Instant};

use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::pdf_evidence_common::{
    error_result, json_result as text_result, parse_pdf_evidence_args,
};
use crate::provider_budget::MAX_REQUEST_TIMEOUT_MS;
use crate::visual_evidence;

use super::normalize::normalize_output;
use super::provider::{provider_config, run_provider};
use super::request_limits::{
    acquire_permit, refusal, RequestOcrBudget, DEFAULT_MAX_OUTPUT_CHARS, MAX_SOURCES_PER_REQUEST,
};
use super::timeout_refusal;

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
