use serde_json::json;
use std::path::PathBuf;

use super::pdf_evidence;

fn structure_fixture() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-structure-v1.pdf")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn inspect_samples_a_compact_max_u32_range_without_expanding_it_unboundedly() {
    for range in ["1-4294967295", "1--5", "1-   "] {
        let result = pdf_evidence(json!({
            "operation":"inspect",
            "sources":[{"path":structure_fixture(),"pages":range}],
            "sample_pages":20
        }))
        .unwrap();
        let payload = result.structured_content.unwrap();
        assert_eq!(payload["results"][0]["success"], true, "{range}");
        assert_eq!(
            payload["results"][0]["data"]["sampled_pages"],
            json!([1, 2]),
            "{range}"
        );
    }
}

#[test]
fn inspect_rejects_max_plus_one_raw_targets_before_file_io() {
    let result = pdf_evidence(json!({
        "operation":"inspect",
        "sources":[{"path":"definitely-missing.pdf","pages":"1-10001,20000"}],
        "sample_pages":5
    }))
    .unwrap();
    let payload = result.structured_content.unwrap();
    assert_eq!(payload["results"][0]["success"], false);
    assert!(
        payload["results"][0]["error"]
            .as_str()
            .unwrap()
            .contains("10001 selected pages")
    );
}
