use anymd_core::text_index::cite_check::{check_cite_items_with_separator, CiteNormalization};
use serde_json::json;

use crate::schema::InspectArgs;

use super::{
    evidence::ocr_items,
    normalize_args,
    request::{bounds, validate},
};

fn args() -> InspectArgs {
    serde_json::from_value(json!({"operation":"cite_check", "sources":[{"path":"fixture.pdf"}], "citations":[{"quote":"quote", "page":1,"bounding_box":{"left":0,"bottom":0,"right":10,"top":10}}]})).unwrap()
}
#[test]
fn cite_check_ocr_fixture_never_assembles_disconnected_words() {
    let page: anymd_core::OcrPage = serde_json::from_value(json!({
        "page":1,"text":"first missing second", "provider":"fixture",
        "source_render_evidence_id":"fixture", "provenance":{},
        "words":[{"text":"first", "bounding_box":{"left":0,"bottom":0,"right":10,"top":10}},
                 {"text":"second", "bounding_box":{"left":0,"bottom":0,"right":10,"top":10}}]
    }))
    .unwrap();
    let items = ocr_items(&page);
    assert_eq!(items.len(), 1);
    let bbox = bounds(&args().citations.unwrap()[0]);
    assert_eq!(
        check_cite_items_with_separator(
            &items,
            "first",
            bbox,
            CiteNormalization::None,
            false,
            "ocr_word",
            "",
            None
        )
        .verdict,
        "verified_exact"
    );
    assert_eq!(
        check_cite_items_with_separator(
            &items,
            "first second",
            bbox,
            CiteNormalization::WhitespaceV1,
            false,
            "ocr_word",
            "",
            None
        )
        .verdict,
        "insufficient_evidence"
    );
}

#[test]
fn cite_check_bounds_rejected() {
    let mut value = args();
    assert!(validate(&value).is_ok());
    value.citations = Some(vec![args().citations.unwrap().remove(0); 101]);
    assert!(validate(&value).is_err());
    value = args();
    value.citations.as_mut().unwrap()[0].quote.clear();
    assert!(validate(&value).is_err());
    value = args();
    value.citations = Some(vec![
        {
            let mut c = args().citations.unwrap().remove(0);
            c.quote = "x".repeat(4096);
            c
        };
        16
    ]);
    assert!(validate(&value).is_err());
    value = args();
    value.citations.as_mut().unwrap()[0].quote = "😀".repeat(2049);
    assert!(validate(&value).is_err());
    value = args();
    value.citations.as_mut().unwrap()[0]
        .bounding_box
        .as_mut()
        .unwrap()
        .right = 0.;
    assert!(validate(&value).is_err());
    value = args();
    value.citations.as_mut().unwrap()[0].bounding_box = None;
    assert!(validate(&value).is_ok());
    let mut from_read: InspectArgs = serde_json::from_value(json!({
        "operation": "cite_check",
        "sources": [{"path": "fixture.pdf"}],
        "citations": [{"quote": "<!-- page 3 -->\nDeterministic quote"}]
    }))
    .unwrap();
    normalize_args(&mut from_read);
    assert_eq!(from_read.citations.as_ref().unwrap()[0].page, 3);
    assert_eq!(
        from_read.citations.as_ref().unwrap()[0].quote,
        "Deterministic quote"
    );
    assert!(validate(&from_read).is_ok());
    let mut from_search: InspectArgs = serde_json::from_value(json!({
        "operation": "cite_check",
        "sources": [{"path": "fixture.pdf"}],
        "citations": [{"quote": "- p.4: …**Deterministic quote**…"}]
    }))
    .unwrap();
    normalize_args(&mut from_search);
    assert_eq!(from_search.citations.as_ref().unwrap()[0].page, 4);
    assert_eq!(
        from_search.citations.as_ref().unwrap()[0].quote,
        "Deterministic quote"
    );
    assert!(validate(&from_search).is_ok());
    value = args();
    value.citations.as_mut().unwrap()[0].page = 0;
    assert!(validate(&value).is_err());
    value = args();
    value.expected_source_sha256 = Some("bad".into());
    assert!(validate(&value).is_err());
    value = args();
    value.citations = Some(
        (1..=21)
            .map(|page| {
                let mut c = args().citations.unwrap().remove(0);
                c.page = page;
                c
            })
            .collect(),
    );
    assert!(validate(&value).is_err());
}
