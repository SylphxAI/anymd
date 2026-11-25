//! Collect in-scope safety, layout, table, and annotation signals.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use super::super::pattern;
use super::redaction::redact_trust_evidence;

static URL_SCHEME_PATTERN: OnceLock<Regex> = OnceLock::new();

pub(super) struct CollectedSignals {
    pub(super) safety_findings: Vec<Value>,
    pub(super) signals: Vec<Value>,
}

pub(super) fn collect_signals(
    selected: &HashSet<u32>,
    safety: &Value,
    layout: &Value,
    elements: &Value,
    annotations: Option<&Value>,
    redaction_policy: &str,
) -> CollectedSignals {
    let in_scope = |value: &Value| {
        value
            .get("page")
            .and_then(Value::as_u64)
            .is_none_or(|page| u32::try_from(page).is_ok_and(|page| selected.contains(&page)))
    };
    let safety_findings = safety
        .as_array()
        .into_iter()
        .flatten()
        .filter(|finding| in_scope(finding))
        .cloned()
        .collect::<Vec<_>>();
    let mut signals = Vec::new();

    for finding in &safety_findings {
        let mut evidence = serde_json::Map::new();
        if let Some(kind) = finding.get("type").and_then(Value::as_str) {
            evidence.insert("finding_type".into(), json!(kind));
        }
        evidence.insert("redaction_policy".into(), json!(redaction_policy));
        if let Some(bbox) = finding.get("bounding_box") {
            evidence.insert("bounding_box".into(), bbox.clone());
        }
        if let Some(snippet) = finding.get("snippet").and_then(Value::as_str) {
            let (redacted, types) = redact_trust_evidence(snippet, redaction_policy);
            evidence.insert("snippet".into(), json!(redacted));
            if !types.is_empty() {
                evidence.insert("snippet_redacted".into(), json!(true));
                evidence.insert("redaction_types".into(), json!(types));
            } else if redaction_policy == "off" {
                evidence.insert("snippet_redacted".into(), json!(false));
            }
        }
        let mut signal = serde_json::Map::new();
        signal.insert("type".into(), json!("content_safety"));
        signal.insert(
            "severity".into(),
            json!(match finding.get("severity").and_then(Value::as_str) {
                Some("high") => "high",
                Some("medium") => "medium",
                _ => "low",
            }),
        );
        if let Some(page) = finding.get("page").and_then(Value::as_u64) {
            signal.insert("page".into(), json!(page));
        }
        signal.insert(
            "message".into(),
            finding.get("message").cloned().unwrap_or(Value::Null),
        );
        if let Some(id) = finding.get("element_id").and_then(Value::as_str) {
            signal.insert("element_id".into(), json!(id));
        }
        signal.insert("evidence".into(), Value::Object(evidence));
        signals.push(Value::Object(signal));
    }

    for diagnostic in layout.as_array().into_iter().flatten() {
        if !in_scope(diagnostic) {
            continue;
        }
        let page = diagnostic.get("page").and_then(Value::as_u64);
        let confidence = diagnostic
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        if confidence < 0.7 {
            let mut evidence = serde_json::Map::new();
            for key in [
                "profile",
                "reading_order",
                "confidence",
                "signals",
                "warnings",
            ] {
                if let Some(value) = diagnostic.get(key) {
                    evidence.insert(key.into(), value.clone());
                }
            }
            let mut signal = json!({
                "type": "layout_uncertainty",
                "severity": if confidence < 0.5 { "high" } else { "medium" },
                "message": "Page layout confidence is low; verify reading order before using extracted text as evidence.",
                "evidence": evidence,
            });
            if let Some(page) = page {
                signal["page"] = json!(page);
            }
            signals.push(signal);
        }
        if diagnostic.get("profile").and_then(Value::as_str) == Some("image_or_sparse") {
            let text_item_count = diagnostic
                .get("text_item_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let mut signal = json!({
                "type": "sparse_or_scanned",
                "severity": if text_item_count == 0 { "high" } else { "medium" },
                "message": "Page has sparse selectable text; route through OCR or visual evidence before trusting text completeness.",
                "evidence": {
                    "text_item_count": text_item_count,
                    "image_item_count": diagnostic.get("image_item_count").cloned().unwrap_or_else(|| json!(0)),
                    "positioned_item_ratio": diagnostic.get("positioned_item_ratio").cloned().unwrap_or_else(|| json!(0)),
                },
            });
            if let Some(page) = page {
                signal["page"] = json!(page);
            }
            signals.push(signal);
        }
    }

    for element in elements.as_array().into_iter().flatten() {
        if !in_scope(element) || element.get("type").and_then(Value::as_str) != Some("table") {
            continue;
        }
        let Some(quality) = element.pointer("/table/quality") else {
            continue;
        };
        let Some(warnings) = quality.get("warnings").and_then(Value::as_array) else {
            continue;
        };
        if warnings.is_empty() {
            continue;
        }
        let quality_signals = quality
            .get("signals")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let has = |kind: &str| {
            quality_signals
                .iter()
                .any(|value| value.as_str() == Some(kind))
        };
        let severity = if has("low_confidence") {
            "high"
        } else if has("multi_page_continuation_candidate") {
            "low"
        } else {
            "medium"
        };
        for warning in warnings.iter().filter_map(Value::as_str) {
            let mut signal = json!({
                "type": "table_quality",
                "severity": severity,
                "table_id": element.get("id").cloned().unwrap_or(Value::Null),
                "message": warning,
                "evidence": {
                    "confidence": element.pointer("/table/confidence").cloned().unwrap_or(Value::Null),
                    "row_count": element.pointer("/table/rowCount").cloned().unwrap_or(Value::Null),
                    "col_count": element.pointer("/table/colCount").cloned().unwrap_or(Value::Null),
                    "signals": quality_signals,
                    "completeness": quality.get("completeness").cloned().unwrap_or(Value::Null),
                },
            });
            if let Some(page) = element.get("page").and_then(Value::as_u64) {
                signal["page"] = json!(page);
            }
            signals.push(signal);
        }
    }

    for page_annotations in annotations.and_then(Value::as_array).into_iter().flatten() {
        let page = page_annotations.get("page").and_then(Value::as_u64);
        if page
            .is_some_and(|page| u32::try_from(page).map_or(true, |page| !selected.contains(&page)))
        {
            continue;
        }
        for annotation in page_annotations
            .get("annotations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(url) = annotation.get("url").and_then(Value::as_str) else {
                continue;
            };
            let normalized = url.trim().to_lowercase();
            let scheme = pattern(&URL_SCHEME_PATTERN, r"(?i)^[a-z][a-z0-9+.-]*:")
                .find(&normalized)
                .map(|entry| entry.as_str().trim_end_matches(':'));
            let unsafe_url = scheme.is_some_and(|scheme| {
                matches!(scheme, "javascript" | "data" | "file" | "vbscript")
            });
            let mut evidence = serde_json::Map::new();
            if let Some(subtype) = annotation.get("subtype").and_then(Value::as_str) {
                evidence.insert("subtype".into(), json!(subtype));
            }
            evidence.insert("url".into(), json!(url));
            if let Some(bbox) = annotation.get("bounding_box") {
                evidence.insert("bounding_box".into(), bbox.clone());
            }
            let mut signal = json!({
                "type": if unsafe_url { "unsafe_external_link" } else { "external_link" },
                "severity": if unsafe_url { "high" } else { "low" },
                "message": if unsafe_url {
                    "Annotation contains a potentially unsafe URL scheme."
                } else {
                    "Annotation contains an external link; treat link target as untrusted content."
                },
                "evidence": evidence,
            });
            if let Some(page) = page {
                signal["page"] = json!(page);
            }
            if let Some(id) = annotation.get("id").and_then(Value::as_str) {
                signal["annotation_id"] = json!(id);
            }
            signals.push(signal);
        }
    }

    signals.retain(|signal| in_scope(signal));
    CollectedSignals {
        safety_findings,
        signals,
    }
}
