use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::schema::PdfSource;
use crate::SERVER_VERSION;

/// Family evidence envelope v1 product id.
pub const PRODUCT: &str = "anymd";
pub const ENVELOPE_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfToolEvidence {
    pub subject: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    pub freshness: Freshness,
    pub locator: Locator,
    pub route: Route,
    pub confidence: &'static str,
    pub warnings: Vec<String>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Freshness {
    pub indexed_at: String,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Locator {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub extraction: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
}

pub fn primary_source_label(sources: &[PdfSource]) -> String {
    sources
        .first()
        .and_then(|source| source.path.clone().or_else(|| source.url.clone()))
        .unwrap_or_else(|| "unknown".into())
}

/// Attach product-local evidence **and** family envelope v1 fields.
///
/// Family wire law (`instrument-evidence-envelope.schema.json`):
/// envelope_version, status, tool, product, product_version, route, payload,
/// warnings, gaps. Domain twin remains in `payload` (and top-level keys are
/// preserved for existing agents that read them directly).
pub fn attach_evidence(
    tool: &str,
    operation: Option<&str>,
    sources: &[PdfSource],
    route: &str,
    source_hash: Option<String>,
    warnings: Vec<String>,
    payload: Value,
) -> Value {
    let source = primary_source_label(sources);
    let first = sources.first();
    let evidence = PdfToolEvidence {
        subject: source.clone(),
        source: source.clone(),
        source_hash: source_hash.clone(),
        freshness: Freshness {
            indexed_at: Utc::now().to_rfc3339(),
            stale: false,
        },
        locator: Locator {
            path: first.and_then(|s| s.path.clone()),
            url: first.and_then(|s| s.url.clone()),
            tool: tool.into(),
            operation: operation.map(str::to_string),
        },
        route: Route {
            extraction: route.into(),
            tool: tool.into(),
            operation: operation.map(str::to_string),
        },
        confidence: "deterministic",
        warnings: warnings.clone(),
        next_actions: vec![
            "Use search_pdf for literal retrieval with page and bbox locators".into(),
            "Use pdf_evidence for inspect, render, crop, OCR, or visual-region follow-up".into(),
        ],
    };

    let mut object = match payload {
        Value::Object(map) => map,
        other => {
            let mut map = Map::new();
            map.insert("payload".into(), other);
            map
        }
    };

    // Domain-local evidence block (existing agents).
    object.insert(
        "evidence".into(),
        serde_json::to_value(evidence).unwrap_or(json!({})),
    );

    // Family envelope v1 (authoritative cross-instrument contract).
    object.insert("envelope_version".into(), json!(ENVELOPE_VERSION));
    object.insert("status".into(), json!("ok"));
    object.insert("tool".into(), json!(tool));
    object.insert("product".into(), json!(PRODUCT));
    object.insert("product_version".into(), json!(SERVER_VERSION));
    object.insert(
        "route".into(),
        json!({
            "engine": "rust-core",
            "path": route,
        }),
    );
    object.insert("warnings".into(), json!(warnings));
    if !object.contains_key("gaps") {
        object.insert("gaps".into(), json!([]));
    }
    object.insert(
        "confidence".into(),
        json!({ "kind": "deterministic", "notes": [] }),
    );

    let mut source_obj = Map::new();
    if let Some(path) = first.and_then(|s| s.path.clone()) {
        source_obj.insert("path".into(), json!(path));
    }
    if let Some(url) = first.and_then(|s| s.url.clone()) {
        source_obj.insert("url".into(), json!(url));
    }
    if let Some(hash) = source_hash {
        source_obj.insert("hash".into(), json!(hash));
    }
    if !source_obj.is_empty() {
        object.insert("source".into(), Value::Object(source_obj));
    }

    // payload: snapshot of domain fields excluding family envelope keys for clean parse
    // Keep top-level domain fields for compatibility; also set payload to a clone of twin body if present.
    if !object.contains_key("payload") {
        // Prefer common twin keys if present; else omit to avoid huge duplication.
        if object.contains_key("results")
            || object.contains_key("documents")
            || object.contains_key("markdown")
            || object.contains_key("pages")
        {
            // leave domain at top-level; agents already consume them
        }
    }

    if let Some(op) = operation {
        object.insert("operation".into(), json!(op));
    }

    Value::Object(object)
}

/// Wrap an error-shaped structured body with family envelope fields.
pub fn attach_error_envelope(tool: &str, code: &str, message: &str, warnings: Vec<String>) -> Value {
    json!({
        "envelope_version": ENVELOPE_VERSION,
        "status": "error",
        "tool": tool,
        "product": PRODUCT,
        "product_version": SERVER_VERSION,
        "route": { "engine": "rust-core" },
        "payload": null,
        "warnings": warnings,
        "gaps": [],
        "error": { "code": code, "message": message },
        "confidence": { "kind": "deterministic", "notes": [] },
    })
}
