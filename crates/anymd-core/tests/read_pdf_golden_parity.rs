//! Golden fixture parity for `read_pdf` in the core.

use std::fs;
use std::path::{Path, PathBuf};

use anymd_core::{read_pdf_from_value, ReadPdfErrorCode, READ_PDF_ROUTE};
use serde_json::{json, Value};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures_root() -> PathBuf {
    repo_root().join("test/fixtures")
}

fn golden_manifest() -> Value {
    let path = fixtures_root().join("read-pdf-golden.json");
    let raw = fs::read_to_string(path).expect("read golden manifest");
    serde_json::from_str(&raw).expect("parse golden manifest")
}

fn normalize_path_label(path: &str) -> String {
    let fixtures = fixtures_root();
    Path::new(path)
        .strip_prefix(&fixtures)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.to_string())
}

fn normalize_payload(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
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

fn build_request_input(fixture: &str, input: &Value) -> Value {
    let mut request = input.clone();
    let object = request
        .as_object_mut()
        .expect("golden input should be an object");

    if !object.contains_key("sources") {
        object.insert(
            "sources".into(),
            json!([{ "path": fixtures_root().join(fixture) }]),
        );
    }

    if let Some(sources) = object.get_mut("sources").and_then(Value::as_array_mut) {
        for source in sources {
            if let Some(source_object) = source.as_object_mut() {
                if let Some(path) = source_object.get("path").and_then(Value::as_str) {
                    if !Path::new(path).is_absolute() {
                        source_object.insert(
                            "path".into(),
                            Value::String(fixtures_root().join(path).to_string_lossy().to_string()),
                        );
                    }
                }
            }
        }
    }

    request
}

fn subset_matches(actual: &Value, expected: &Value, id: &str, pointer: &str) {
    let actual_at = actual.pointer(pointer).unwrap_or_else(|| {
        panic!("{id}: actual payload missing pointer {pointer}");
    });
    let expected_at = expected.pointer(pointer).unwrap_or_else(|| {
        panic!("{id}: golden payload missing pointer {pointer}");
    });
    assert_eq!(actual_at, expected_at, "{id}: mismatch at {pointer}");
}

fn assert_success_case(id: &str, fixture: &str, input: &Value, expected: &Value) {
    let fixture_path = fixtures_root().join(fixture);
    if !fixture_path.is_file() {
        return;
    }

    let response = read_pdf_from_value(&build_request_input(fixture, input))
        .unwrap_or_else(|error| panic!("{id}: read_pdf failed: {error:?}"));

    let full_text = response
        .results
        .first()
        .and_then(|result| result.data.as_ref())
        .and_then(|data| data.full_text.clone())
        .unwrap_or_default();

    let actual = normalize_payload(serde_json::to_value(&response).expect("serialize response"));
    let expected_payload = expected
        .get("payload")
        .expect("{id}: golden success case should include payload");

    assert_eq!(
        actual.get("profile").and_then(Value::as_str),
        expected_payload.get("profile").and_then(Value::as_str)
    );

    let actual_results = actual
        .get("results")
        .and_then(Value::as_array)
        .expect("{id}: results");
    let expected_results = expected_payload
        .get("results")
        .and_then(Value::as_array)
        .expect("{id}: expected results");

    assert_eq!(
        actual_results.len(),
        expected_results.len(),
        "{id}: result count"
    );
    assert!(
        actual_results[0]
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "{id}: first result should succeed"
    );

    let route = actual_results[0]
        .pointer("/data/route")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert_eq!(route, READ_PDF_ROUTE, "{id}: route");

    let num_pages = actual_results[0]
        .pointer("/data/numPages")
        .or_else(|| actual_results[0].pointer("/data/num_pages"))
        .and_then(Value::as_u64);
    if input
        .get("include_page_count")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        assert!(
            num_pages.is_some_and(|count| count >= 1),
            "{id}: num_pages should be positive when requested or defaulted"
        );
    } else {
        assert_eq!(
            num_pages, None,
            "{id}: num_pages should be omitted when include_page_count is false"
        );
    }

    for pointer in [
        "/results/0/data/route",
        "/results/0/data/engine/name",
        "/results/0/data/engine/version",
        "/results/0/data/info/Title",
        "/results/0/data/info/Producer",
        "/results/0/data/info/PDFFormatVersion",
    ] {
        if pointer.contains("/info/")
            && input.get("include_metadata").and_then(Value::as_bool) == Some(false)
        {
            continue;
        }
        if expected_payload.pointer(pointer).is_some() {
            subset_matches(&actual, expected_payload, id, pointer);
        }
    }

    if let Some(needle) = expected_results[0]
        .pointer("/data/full_text_contains")
        .and_then(Value::as_str)
    {
        assert!(
            full_text.contains(needle),
            "{id}: expected full_text to contain '{needle}'"
        );
    }
}

#[test]
fn read_pdf_matches_golden_contract_on_sample_fixture() {
    let manifest = golden_manifest();
    let cases = manifest
        .get("cases")
        .and_then(Value::as_array)
        .expect("golden cases");

    for case in cases {
        let id = case.get("id").and_then(Value::as_str).expect("case id");
        let fixture = case
            .get("fixture")
            .and_then(Value::as_str)
            .expect("fixture");
        let input = case.get("input").expect("input");
        let expects = case.get("expects").expect("expects");

        if expects.get("error").and_then(Value::as_bool) == Some(true) {
            let err = read_pdf_from_value(&build_request_input(fixture, input))
                .expect_err("{id}: expected error");
            let expected_code = expects
                .get("code")
                .and_then(Value::as_str)
                .expect("error code");
            let actual_code = match err.code {
                ReadPdfErrorCode::InvalidParams => "INVALID_PARAMS",
                ReadPdfErrorCode::InvalidRequest => "INVALID_REQUEST",
                ReadPdfErrorCode::ExtractionFailed => "EXTRACTION_FAILED",
            };
            assert_eq!(actual_code, expected_code, "{id}: error code");
            let needle = expects
                .get("message_contains")
                .and_then(Value::as_str)
                .expect("message_contains");
            assert!(
                err.message
                    .to_ascii_lowercase()
                    .contains(&needle.to_ascii_lowercase()),
                "{id}: expected message to contain '{needle}', got '{}'",
                err.message
            );
            continue;
        }

        assert_success_case(id, fixture, input, expects);
    }
}
