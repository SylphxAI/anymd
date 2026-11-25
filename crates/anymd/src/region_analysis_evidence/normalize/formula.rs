//! Formula representations and their provider aliases.

use serde_json::{json, Map, Value};

use super::scalar::{confidence, normalized_string};

pub(super) fn normalize_formula(value: Option<&Value>, maximum: usize) -> Option<Value> {
    let candidate = value?.as_object()?;
    let mut output = Map::new();
    for (source, target) in [
        ("latex", "latex"),
        ("mathml", "mathml"),
        ("asciimath", "asciimath"),
        ("text", "text"),
    ] {
        if let Some(value) = normalized_string(candidate.get(source), maximum) {
            output.insert(target.into(), json!(value));
        }
    }
    if !output.contains_key("asciimath") {
        if let Some(value) = normalized_string(candidate.get("ascii_math"), maximum) {
            output.insert("asciimath".into(), json!(value));
        }
    }
    if let Some(value) = confidence(candidate.get("confidence")) {
        output.insert("confidence".into(), json!(value));
    }
    (!output.is_empty()).then_some(Value::Object(output))
}
