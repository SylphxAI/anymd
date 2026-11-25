use serde_json::{json, Value};

use super::walk::{MAX_REQUEST_WORK, PDFJS_MAX_DEPTH};

pub(super) fn normalize_public_tree(raw: &Value) -> Option<Value> {
    fn node(raw: &Value, depth: usize, remaining: &mut usize) -> Option<Value> {
        if depth > PDFJS_MAX_DEPTH || *remaining == 0 {
            return None;
        }
        *remaining -= 1;
        let object = raw.as_object()?;
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Unknown");
        let mut children = Vec::new();
        for child in object
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(object) = child.as_object() else {
                continue;
            };
            if object.contains_key("role") || object.contains_key("children") {
                if let Some(child) = node(child, depth + 1, remaining) {
                    children.push(child);
                }
                continue;
            }
            let kind = object
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if kind.is_empty() && id.is_empty() {
                continue;
            }
            let mut value = json!({"type":if kind.is_empty(){"content"}else{kind}});
            if !id.is_empty() {
                value["id"] = json!(id);
            }
            children.push(value);
        }
        let mut value = json!({"role":role});
        if !children.is_empty() {
            value["children"] = json!(children);
        }
        Some(value)
    }
    let mut remaining = MAX_REQUEST_WORK;
    node(raw, 0, &mut remaining)
}
