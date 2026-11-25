//! Candidate region inputs and system-owned enrichment metadata.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::visual_evidence;

use super::provider_config::ProviderConfig;

pub(super) fn regions_for_crop(candidates: &[Value]) -> Vec<Value> {
    candidates
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
        .collect::<Vec<_>>()
}

pub(super) fn crop_candidate_regions(crop_input: Value) -> Result<(Value, Value), String> {
    let cropped =
        visual_evidence::extract_regions(crop_input).map_err(|error| error.message.to_string())?;
    let encoded = serde_json::to_value(&cropped)
        .map_err(|error| format!("Failed to encode crop evidence: {error}"))?;
    let Some(payload) = cropped.structured_content else {
        let message = encoded["content"][0]["text"]
            .as_str()
            .unwrap_or("All PDF sources failed region analysis.")
            .replace("failed region extraction", "failed region analysis");
        let message = message
            .strip_prefix("All PDF sources failed region analysis: ")
            .unwrap_or(&message)
            .to_string();
        return Err(message);
    };
    let source = payload["results"]
        .as_array()
        .and_then(|results| results.first())
        .cloned()
        .unwrap_or_else(|| json!({"success": false, "error": "missing crop result"}));
    if source["success"] != true {
        return Err(source["error"]
            .as_str()
            .unwrap_or("Region crop failed.")
            .to_string());
    }
    Ok((source, encoded))
}

pub(super) fn index_candidates(candidates: &[Value]) -> BTreeMap<String, &Value> {
    candidates
        .iter()
        .filter_map(|candidate| {
            let id = candidate
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| candidate.pointer("/region/id").and_then(Value::as_str))?;
            Some((id.to_string(), candidate))
        })
        .collect::<BTreeMap<_, _>>()
}

pub(super) fn attach_candidate_metadata(
    normalized: &mut Value,
    region: &Value,
    candidates_by_id: &BTreeMap<String, &Value>,
    config: &ProviderConfig,
) {
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
}
