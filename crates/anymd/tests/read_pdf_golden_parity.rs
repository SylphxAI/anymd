//! rmcp `read_pdf` handler parity against anymd-core golden payloads.

use std::path::{Path, PathBuf};

use anymd_core::{read_pdf_from_value, READ_PDF_ROUTE};
use anymd::read_pdf;
use serde_json::{json, Value};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures_root() -> PathBuf {
    repo_root().join("test/fixtures")
}

fn normalize_path_label(path: &str) -> String {
    let fixtures = fixtures_root();
    Path::new(path)
        .strip_prefix(&fixtures)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.to_string())
}

fn normalize_structured(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("evidence");
        // Family envelope v1 fields are MCP-boundary additions, not core payload.
        for key in [
            "envelope_version",
            "status",
            "tool",
            "product",
            "product_version",
            "route",
            "warnings",
            "gaps",
            "confidence",
            "source",
            "operation",
            "payload",
        ] {
            object.remove(key);
        }
        if let Some(results) = object.get_mut("results").and_then(Value::as_array_mut) {
            for result in results {
                if let Some(result_object) = result.as_object_mut() {
                    if let Some(source) = result_object
                        .get("source")
                        .and_then(Value::as_str)
                        .map(normalize_path_label)
                    {
                        result_object.insert("source".into(), Value::String(source));
                    }
                    if let Some(data) = result_object.get_mut("data").and_then(Value::as_object_mut)
                    {
                        data.remove("full_text");
                        if let Some(info) = data.get_mut("info").and_then(Value::as_object_mut) {
                            info.remove("text_chars");
                        }
                    }
                }
            }
        }
    }
    value
}

fn normalize_evidence(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("sourceHash");
        if let Some(freshness) = object.get_mut("freshness").and_then(Value::as_object_mut) {
            freshness.insert("indexedAt".into(), Value::String("NORMALIZED".into()));
        }
        for key in ["subject", "source"] {
            if let Some(path) = object.get(key).and_then(Value::as_str) {
                object.insert(key.into(), Value::String(normalize_path_label(path)));
            }
        }
        if let Some(locator) = object.get_mut("locator").and_then(Value::as_object_mut) {
            if let Some(path) = locator.get("path").and_then(Value::as_str) {
                locator.insert("path".into(), Value::String(normalize_path_label(path)));
            }
        }
    }
    value
}

#[test]
fn rmcp_read_pdf_structured_content_matches_core_payload() {
    let fixture = fixtures_root().join("sample.pdf");
    if !fixture.is_file() {
        return;
    }

    let args = json!({
        "sources": [{ "path": fixture }],
        "include_metadata": true,
        "include_page_count": true,
        "include_full_text": false
    });

    let core = read_pdf_from_value(&args).expect("core read_pdf");
    let rmcp = read_pdf::read_pdf(args).expect("rmcp read_pdf");
    let structured = rmcp
        .structured_content
        .expect("structured content should be present");

    assert_eq!(
        normalize_structured(structured.clone()),
        normalize_structured(serde_json::to_value(core).expect("core payload"))
    );

    assert_eq!(
        structured.get("envelope_version").and_then(Value::as_str),
        Some("1")
    );
    assert_eq!(
        structured.get("product").and_then(Value::as_str),
        Some("anymd")
    );
    assert_eq!(
        structured
            .pointer("/route/engine")
            .and_then(Value::as_str),
        Some("rust-core")
    );

    let evidence = structured.get("evidence").cloned().expect("rmcp evidence");
    assert_eq!(
        evidence
            .pointer("/route/extraction")
            .and_then(Value::as_str),
        Some(READ_PDF_ROUTE)
    );
    assert_eq!(
        evidence.pointer("/locator/tool").and_then(Value::as_str),
        Some("read_pdf")
    );
    assert_eq!(
        evidence.get("confidence").and_then(Value::as_str),
        Some("deterministic")
    );

    let normalized_evidence = normalize_evidence(evidence);
    assert_eq!(
        normalized_evidence
            .pointer("/route/tool")
            .and_then(Value::as_str),
        Some("read_pdf")
    );
    assert_eq!(
        normalized_evidence
            .pointer("/route/extraction")
            .and_then(Value::as_str),
        Some(READ_PDF_ROUTE)
    );
}
