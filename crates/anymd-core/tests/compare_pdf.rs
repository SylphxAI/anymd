use anymd_core::compare_pdf_from_value;
use serde_json::json;

#[test]
fn rejects_the_same_document_and_requires_distinct_sources() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-selectable-table-v1.pdf");
    let error = compare_pdf_from_value(&json!({ "before": fixture, "after": fixture }))
        .expect_err("same source must fail");
    assert!(error.message.contains("different"));
}
