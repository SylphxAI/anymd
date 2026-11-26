use super::*;

fn sample_fixture() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/sample.pdf")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn rejects_ocr_source_cap_plus_one_before_source_io() {
    admit_ocr_source_count(MAX_OCR_SOURCES_PER_REQUEST).expect("exact source cap");
    let sources = (0..=MAX_OCR_SOURCES_PER_REQUEST)
        .map(|index| json!({"path": format!("/must-not-touch-{index}.pdf")}))
        .collect::<Vec<_>>();
    let error = search_pdf(json!({
        "sources": sources,
        "query": "x",
        "include_ocr_text_layer": true,
    }))
    .expect_err("source cap plus one");
    assert!(error.message.contains("at most 32 sources"));
}

#[test]
fn rejects_global_options_before_source_io() {
    let error = search_pdf(json!({
        "sources": [{"path": "/must-not-touch-invalid-search-options.pdf"}],
        "query": "x",
        "include_ocr_text_layer": true,
        "max_pages": 0,
    }))
    .expect_err("invalid global option");
    assert!(error.message.contains("max_pages"));
    assert!(!error.message.contains("must-not-touch"));
}

#[test]
fn keeps_invalid_page_spec_source_local_before_materialization() {
    let response = search_pdf(json!({
        "sources": [
            {"path": sample_fixture(), "pages": [1]},
            {"path": "/must-not-touch-invalid-page-spec.pdf", "pages": "abc"}
        ],
        "query": "Lorem",
        "include_ocr_text_layer": true,
    }))
    .expect("mixed source request remains successful");
    let structured = response.structured_content.expect("structured result");
    let results = structured["results"].as_array().expect("results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["success"], json!(true));
    assert_eq!(results[1]["success"], json!(false));
    let error = results[1]["error"].as_str().expect("source-local error");
    assert!(error.contains("Invalid page number: abc"));
    assert!(error.contains("must-not-touch-invalid-page-spec.pdf"));
}

#[test]
fn all_invalid_ocr_sources_return_a_tool_error_not_protocol_error() {
    let response = search_pdf(json!({
        "sources": [{"path": "/must-not-touch-all-invalid.pdf", "pages": "abc"}],
        "query": "x",
        "include_ocr_text_layer": true,
    }))
    .expect("all-source failure remains a tools/call result");
    assert_eq!(response.is_error, Some(true));
    assert_eq!(response.content.len(), 1);
    let encoded = serde_json::to_value(response).expect("serialize tool error");
    assert_eq!(encoded["isError"], json!(true));
    assert!(encoded["content"][0]["text"]
        .as_str()
        .is_some_and(|message| message.starts_with("All PDF sources failed search: ")));
}
