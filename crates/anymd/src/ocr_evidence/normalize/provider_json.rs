use serde_json::{json, Value};

use crate::pdf_evidence_common::truncate_utf16;

use super::words::{normalize_confidence, normalize_words};

pub(super) fn normalize_json_or_text(
    stdout: &str,
    max_output_chars: usize,
    languages: &[String],
    scale: f64,
) -> Value {
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
    let mut provenance = serde_json::Map::new();
    for key in ["model_revision", "device", "quantization"] {
        if let Some(value) = parsed
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
        {
            provenance.insert(key.into(), json!(value));
        }
    }
    if !provenance.is_empty() {
        output["provenance"] = json!(provenance);
    }
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
    let mut warnings = Vec::new();
    if truncated {
        warnings.push(format!(
            "OCR output truncated to {max_output_chars} characters."
        ));
    }
    if let Some(stops) = parsed
        .as_ref()
        .and_then(|value| value.get("truncated"))
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
    {
        warnings.push(format!(
            "Doc-VLM generation stopped in {stops} regions (token limit or repetition)."
        ));
    }
    if !warnings.is_empty() {
        output["warnings"] = json!(warnings);
    }
    output
}
