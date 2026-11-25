use crate::SourceOcrOutcome;
use serde_json::{json, Value};

use super::ocr_fusion::{OcrSearchBudget, MAX_OCR_SEARCH_UTF16_UNITS, MAX_OCR_SEARCH_WORDS};
use super::*;

fn search_response(query: &str, max_matches: usize, existing: Vec<Value>) -> SearchPdfResponse {
    SearchPdfResponse {
        profile: "pdf_search_results",
        search_options: json!({
            "query": query,
            "case_sensitive": false,
            "whole_word": false,
            "max_matches_per_source": max_matches,
            "context_chars": 2,
            "include_ocr_text_layer": true,
        }),
        results: vec![json!({
            "source": "fixture.pdf",
            "success": true,
            "num_pages": 1,
            "searched_pages": [1],
            "total_matches": existing.len(),
            "matches": existing,
        })],
    }
}

fn ocr_page(text: &str, words: Option<Vec<crate::OcrWord>>) -> crate::OcrPage {
    crate::OcrPage {
        page: 1,
        text: text.into(),
        confidence: None,
        words,
        language: None,
        provider: "command".into(),
        source_render_evidence_id: "page-1-render-scale-2".into(),
        source_render_scale: Some(2.0),
        source_render_width: Some(100),
        source_render_height: Some(100),
        provenance: json!({"engine": "external-command", "source": "ocr-provider"}),
        warnings: None,
    }
}

fn outcome(page: crate::OcrPage) -> SourceOcrOutcome {
    SourceOcrOutcome {
        source_index: 0,
        pages: vec![page],
        warnings: Vec::new(),
        error: None,
    }
}

#[test]
fn searches_fixture_with_rust_text_index() {
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
    if !fixture.is_file() {
        return;
    }

    let response = search_pdf(&SearchPdfInput {
        sources: vec![SearchPdfSource {
            path: Some(fixture.to_string_lossy().to_string()),
            url: None,
            pages: None,
        }],
        query: "Lorem".into(),
        ..SearchPdfInput::default()
    })
    .expect("search fixture");

    assert_eq!(response.profile, "pdf_search_results");
    assert!(response.results[0]
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false));
    assert_eq!(
        response.results[0].get("route").and_then(Value::as_str),
        Some(SEARCH_PDF_ROUTE)
    );
    assert!(response.results[0].get("warnings").is_none());
    assert!(response.results[0].get("truncated").is_none());
}

#[test]
fn rejects_descending_and_invalid_page_specs() {
    for pages in [
        json!("5-3"),
        json!("0"),
        json!(""),
        json!([]),
        json!([1, 0]),
    ] {
        let error = crate::pages::parse_page_spec::<SearchPdfError>(&Some(pages), false)
            .expect_err("invalid page spec");
        assert_eq!(error.code, SearchPdfErrorCode::InvalidParams);
    }
    assert_eq!(
        crate::pages::parse_page_spec::<SearchPdfError>(&Some(json!("1x")), false)
            .expect("TS parseInt-compatible page"),
        Some(vec![1])
    );
}

#[test]
fn enforces_search_limits_at_core_boundary() {
    let source = SearchPdfSource {
        path: Some("/does/not/matter.pdf".into()),
        url: None,
        pages: None,
    };
    for input in [
        SearchPdfInput {
            sources: vec![source.clone()],
            query: "x".into(),
            max_pages: Some(0),
            ..Default::default()
        },
        SearchPdfInput {
            sources: vec![source.clone()],
            query: "x".into(),
            max_matches_per_source: Some(0),
            ..Default::default()
        },
    ] {
        let error = search_pdf(&input).expect_err("zero limit must fail");
        assert_eq!(error.code, SearchPdfErrorCode::InvalidParams);
    }
}

#[test]
fn page_ranges_use_only_the_exact_second_hyphen_segment_like_ts() {
    let open = crate::pages::parse_page_spec::<SearchPdfError>(&Some(json!("1--5")), false)
        .expect("TS treats an empty second segment as open")
        .expect("page filter");
    assert_eq!(open.len(), crate::pages::MAX_SELECTED_PAGES);
    assert_eq!(open.first(), Some(&1));
    assert_eq!(open.last(), Some(&10_001));
    assert_eq!(
        crate::pages::parse_page_spec::<SearchPdfError>(&Some(json!("1-2-999")), false)
            .expect("third segment ignored"),
        Some(vec![1, 2])
    );
    let whitespace_open =
        crate::pages::parse_page_spec::<SearchPdfError>(&Some(json!("1-   ")), false)
            .expect("TS trims the whole range part before splitting")
            .expect("page filter");
    assert_eq!(whitespace_open, open);
}

#[test]
fn accepts_whitespace_query_like_v3_0_14_schema_and_handler() {
    let error = search_pdf(&SearchPdfInput {
        sources: vec![SearchPdfSource {
            path: Some("/missing.pdf".into()),
            url: None,
            pages: None,
        }],
        query: " ".into(),
        ..Default::default()
    })
    .expect_err("missing source should fail after query validation");
    assert_eq!(error.code, SearchPdfErrorCode::InvalidRequest);
}

#[test]
fn page_filter_is_applied_before_search_success() {
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
    if !fixture.is_file() {
        return;
    }
    let error = search_pdf(&SearchPdfInput {
        sources: vec![SearchPdfSource {
            path: Some(fixture.to_string_lossy().to_string()),
            url: None,
            pages: Some(json!([9999])),
        }],
        query: "Lorem".into(),
        ..Default::default()
    })
    .expect_err("out-of-range selection must not search page one");
    assert!(error.message.contains("No valid pages to search"));
}

#[test]
fn invalid_page_warning_matches_v3_0_14_wording() {
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
    if !fixture.is_file() {
        return;
    }
    let response = search_pdf(&SearchPdfInput {
        sources: vec![SearchPdfSource {
            path: Some(fixture.to_string_lossy().to_string()),
            url: None,
            pages: Some(json!([1, 99])),
        }],
        query: "Lorem".into(),
        ..Default::default()
    })
    .expect("search fixture");

    assert_eq!(
        response.results[0]["warnings"],
        json!(["Requested page numbers 99 exceed total pages (1)."])
    );
}

#[test]
fn ocr_words_override_page_text_and_preserve_utf16_geometry() {
    let words = vec![
        crate::OcrWord {
            text: "Metric".into(),
            confidence: None,
            reading_order: None,
            region_type: None,
            layout_confidence: None,
            bounding_box: Some(json!({"left":40.0,"bottom":700.0,"right":88.0,"top":710.0})),
        },
        crate::OcrWord {
            text: "Value".into(),
            confidence: None,
            reading_order: None,
            region_type: None,
            layout_confidence: None,
            bounding_box: Some(json!({"left":120.0,"bottom":700.0,"right":202.0,"top":710.0})),
        },
        crate::OcrWord {
            text: "Revenue".into(),
            confidence: None,
            reading_order: None,
            region_type: None,
            layout_confidence: None,
            bounding_box: Some(json!({"left":40.0,"bottom":680.0,"right":100.0,"top":690.0})),
        },
        crate::OcrWord {
            text: "24%".into(),
            confidence: None,
            reading_order: None,
            region_type: None,
            layout_confidence: None,
            bounding_box: Some(json!({"left":112.0,"bottom":680.0,"right":184.0,"top":690.0})),
        },
    ];
    let mut response = search_response("Revenue 24%", 50, Vec::new());
    fuse_search_ocr_outcomes(
        &mut response,
        vec![outcome(ocr_page("ignored", Some(words)))],
    )
    .expect("fuse OCR search");

    let item = &response.results[0]["matches"][0];
    assert_eq!(item["id"], json!("p1-ocr-match-1"));
    assert_eq!(item["match_start"], json!(13));
    assert_eq!(item["match_end"], json!(24));
    assert_eq!(item["snippet"], json!("...e Revenue 24%"));
    assert_eq!(item["ocr_word_index"], json!(2));
    assert_eq!(
        item["bounding_box"],
        json!({"left":40.0,"bottom":680.0,"right":184.0,"top":690.0})
    );
    assert_eq!(item["bounding_box_level"], json!("ocr_word"));
    assert!(item.get("text_item_index").is_none());
}

#[test]
fn ocr_text_fallback_has_no_word_geometry() {
    let mut response = search_response("rocket", 50, Vec::new());
    response.search_options["context_chars"] = json!(1);
    fuse_search_ocr_outcomes(
        &mut response,
        vec![outcome(ocr_page("Astral 🚀 rocket", None))],
    )
    .expect("fuse text-only OCR");
    let item = &response.results[0]["matches"][0];
    assert_eq!(item["match_start"], json!(10));
    assert_eq!(item["match_end"], json!(16));
    assert!(item.get("ocr_word_index").is_none());
    assert!(item.get("bounding_box").is_none());
    assert!(item.get("bounding_box_level").is_none());
}

#[test]
fn prefer_speed_strips_geometry_and_emits_route_warning() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-behavior-v1.pdf");
    let response = search_pdf(&SearchPdfInput {
        sources: vec![SearchPdfSource {
            path: Some(fixture.to_string_lossy().into_owned()),
            url: None,
            pages: None,
        }],
        query: "Needle".into(),
        case_sensitive: None,
        whole_word: None,
        include_ocr_text_layer: None,
        max_pages: Some(10),
        max_matches_per_source: Some(20),
        context_chars: Some(4),
        prefer_speed: Some(true),
    })
    .expect("prefer_speed search");
    assert_eq!(response.search_options["prefer_speed"], json!(true));
    let result = &response.results[0];
    assert_eq!(result["success"], json!(true));
    let warnings = result["warnings"].as_array().expect("warnings");
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .is_some_and(|s| { s.contains("Bounding boxes are unavailable on this route") })),
        "missing speed-route warning: {warnings:?}"
    );
    let matches = result["matches"].as_array().expect("matches");
    assert!(!matches.is_empty());
    for item in matches {
        assert!(item.get("bounding_box").is_none(), "{item}");
        assert!(item.get("bounding_box_level").is_none(), "{item}");
        assert!(item.get("text_item_index").is_none(), "{item}");
        assert_eq!(item["provenance"]["engine"], json!("rust-text-index"));
        assert_eq!(item["provenance"]["source"], json!("text-content"));
    }
}

#[test]
fn ocr_appends_after_selectable_and_only_cap_plus_one_truncates() {
    let selectable = json!({"id":"p1-match-1","page":1,"text":"x"});
    let mut exact = search_response("x", 2, vec![selectable.clone()]);
    fuse_search_ocr_outcomes(&mut exact, vec![outcome(ocr_page("x", None))]).expect("exact cap");
    assert_eq!(
        exact.results[0]["matches"][1]["id"],
        json!("p1-ocr-match-2")
    );
    assert!(exact.results[0].get("truncated").is_none());

    let mut overflow = search_response("x", 2, vec![selectable]);
    fuse_search_ocr_outcomes(&mut overflow, vec![outcome(ocr_page("x x", None))])
        .expect("cap plus one");
    assert_eq!(overflow.results[0]["matches"].as_array().unwrap().len(), 2);
    assert_eq!(overflow.results[0]["truncated"], json!(true));
    assert_eq!(
        overflow.results[0]["warnings"],
        json!(["Search results truncated to 2 matches for this source."])
    );
}

#[test]
fn ocr_failure_is_nonfatal_and_preserves_warning_order() {
    let mut response = search_response("x", 2, Vec::new());
    response.results[0]["warnings"] = json!(["earlier warning"]);
    fuse_search_ocr_outcomes(
        &mut response,
        vec![SourceOcrOutcome {
            source_index: 0,
            pages: Vec::new(),
            warnings: vec!["renderer warning".into()],
            error: Some("provider failed".into()),
        }],
    )
    .expect("soft provider failure");
    assert_eq!(response.results[0]["success"], json!(true));
    assert_eq!(
        response.results[0]["warnings"],
        json!([
            "earlier warning",
            "renderer warning",
            "OCR search unavailable: provider failed"
        ])
    );
}

#[test]
fn ocr_search_budget_accepts_exact_caps_and_sticks_after_plus_one() {
    let mut word_budget = OcrSearchBudget::default();
    word_budget
        .charge(MAX_OCR_SEARCH_WORDS, 0)
        .expect("exact word cap");
    let error = word_budget.charge(1, 0).expect_err("word cap plus one");
    assert_eq!(
        error,
        "Request exceeds OCR search work limit of 250000 words."
    );
    assert_eq!(word_budget.charge(0, 0), Err(error));

    let mut text_budget = OcrSearchBudget::default();
    text_budget
        .charge(0, MAX_OCR_SEARCH_UTF16_UNITS)
        .expect("exact text cap");
    let error = text_budget.charge(0, 1).expect_err("text cap plus one");
    assert_eq!(
        error,
        "Request exceeds OCR search work limit of 2000000 UTF-16 units."
    );
    assert_eq!(text_budget.charge(0, 0), Err(error));
}
