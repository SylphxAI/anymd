use super::{PdfEvidenceArgs, ReadPdfArgs, SearchPdfArgs};
use serde::de::DeserializeOwned;
use serde_json::Value;

fn parse_and_validate<T>(value: Value, validate: impl FnOnce(&T) -> Result<(), String>) -> bool
where
    T: DeserializeOwned,
{
    serde_json::from_value::<T>(value)
        .ok()
        .is_some_and(|parsed| validate(&parsed).is_ok())
}

fn oracle() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test/fixtures/legacy-tool-input-cases.json"
    )))
    .expect("parse legacy tool input cases")
}

fn accepts(tool: &str, args: Value) -> bool {
    match tool {
        "read_pdf" => parse_and_validate::<ReadPdfArgs>(args, ReadPdfArgs::validate),
        "search_pdf" => parse_and_validate::<SearchPdfArgs>(args, SearchPdfArgs::validate),
        "pdf_evidence" => parse_and_validate::<PdfEvidenceArgs>(args, PdfEvidenceArgs::validate),
        other => panic!("unknown oracle tool {other}"),
    }
}

#[test]
fn immutable_v3_0_14_accept_reject_corpus_matches() {
    let oracle = oracle();
    assert_eq!(oracle["tag"], "v3.0.14");
    assert_eq!(oracle["commit"], "92651c79c6ce8d10dfa3c76332176c26f222bd78");
    for case in oracle["cases"].as_array().expect("oracle cases") {
        let id = case["id"].as_str().expect("case id");
        let tool = case["tool"].as_str().expect("case tool");
        let expected = case["accept"].as_bool().expect("case acceptance");
        let actual = accepts(tool, case["args"].clone());
        assert_eq!(actual, expected, "v3.0.14 contract case {id}");
    }
}

mod provider_schema_compat_tests {
    use crate::schema::{PdfSource, ReadArgs, SearchPdfArgs};
    use schemars::schema_for;

    // Legacy names are callable but are not included in tools/list.
    #[test]
    fn legacy_search_optional_overrides_are_not_required() {
        let schema = serde_json::to_value(schema_for!(SearchPdfArgs)).expect("schema json");
        assert_eq!(schema["required"], serde_json::json!(["sources", "query"]));
        let args: SearchPdfArgs = serde_json::from_value(serde_json::json!({
            "sources": [{"path": "report.pdf"}],
            "query": "needle"
        }))
        .expect("optional search arguments can be omitted");
        assert!(args.validate().is_ok());
        for key in ["prefer_speed", "detail"] {
            assert_eq!(
                schema["properties"][key]["type"],
                serde_json::json!(["boolean", "null"])
            );
        }
    }

    #[test]
    fn read_node_keeps_its_own_description() {
        let schema = serde_json::to_value(schema_for!(ReadArgs)).unwrap();
        let properties = &schema["properties"];
        let node = properties["node"]["description"].as_str().unwrap_or("");
        assert!(
            node.contains("section id returned by outline"),
            "{properties}"
        );
        let timeline = properties["timeline"]["description"].as_str().unwrap_or("");
        assert!(timeline.contains("video"), "{properties}");
        assert!(!timeline.contains("section id returned by outline"));
    }
    #[test]
    fn pdf_source_schema_omits_not_required_xor() {
        let schema = schema_for!(PdfSource);
        let json = serde_json::to_string(&schema).expect("serialize schema");
        assert!(
            !json.contains("\"not\""),
            "PdfSource tools/list schema must not emit not/required XOR for provider compat (#562): {json}"
        );
        assert!(
            !json.contains("\"oneOf\""),
            "PdfSource tools/list schema must not emit oneOf XOR for provider compat (#562): {json}"
        );
    }

    #[test]
    fn pdf_source_runtime_still_enforces_exactly_one_locator() {
        assert!(PdfSource {
            path: Some("a.pdf".into()),
            url: None,
            pages: None,
        }
        .validate()
        .is_ok());
        assert!(PdfSource {
            path: None,
            url: Some("https://example.com/a.pdf".into()),
            pages: None,
        }
        .validate()
        .is_ok());
        assert!(PdfSource {
            path: None,
            url: None,
            pages: None,
        }
        .validate()
        .is_err());
        assert!(PdfSource {
            path: Some("a.pdf".into()),
            url: Some("https://example.com/a.pdf".into()),
            pages: None,
        }
        .validate()
        .is_err());
    }
}
