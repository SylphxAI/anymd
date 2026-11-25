//! Normalization of region-analysis provider output into the public kind, table, formula and chart shapes.

use serde_json::{json, Map, Value};

use crate::pdf_evidence_common::truncate_utf16;

mod chart;
mod formula;
mod scalar;
mod table;

use self::{
    chart::normalize_chart,
    formula::normalize_formula,
    scalar::{confidence, normalized_string},
    table::normalize_table,
};

pub(super) fn normalize_output(stdout: &str, maximum: usize) -> Value {
    let trimmed = stdout.trim();
    let parsed = serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object);
    let Some(parsed) = parsed else {
        let (description, truncated) = truncate_utf16(trimmed, maximum);
        let mut output = json!({"kind": "unknown", "description": description});
        if truncated {
            output["warnings"] = json!([format!(
                "Region analysis output truncated to {maximum} characters."
            )]);
        }
        return output;
    };
    let object = parsed.as_object().expect("checked object");
    let mut warnings = object
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|warning| {
            warning
                .as_str()
                .map(str::trim)
                .filter(|warning| !warning.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "unknown".into());
    let kind = if matches!(
        kind.as_str(),
        "text" | "table" | "figure" | "chart" | "formula" | "image" | "diagram" | "unknown"
    ) {
        kind
    } else {
        warnings.push(format!(
            "Unsupported region analysis kind \"{kind}\"; normalized to \"unknown\"."
        ));
        "unknown".into()
    };
    let mut output = Map::from_iter([("kind".into(), json!(kind))]);
    for key in ["description", "text", "markdown"] {
        if let Some(value) = normalized_string(object.get(key), maximum) {
            output.insert(key.into(), json!(value));
        }
    }
    if let Some(value) = confidence(object.get("confidence")) {
        output.insert("confidence".into(), json!(value));
    }
    if let Some(value) = normalize_table(object.get("table"), maximum) {
        output.insert("table".into(), value);
    }
    if let Some(value) = normalize_formula(object.get("formula"), maximum) {
        output.insert("formula".into(), value);
    }
    if let Some(value) = normalize_chart(object.get("chart"), maximum) {
        output.insert("chart".into(), value);
    }
    if !warnings.is_empty() {
        output.insert("warnings".into(), json!(warnings));
    }
    Value::Object(output)
}
