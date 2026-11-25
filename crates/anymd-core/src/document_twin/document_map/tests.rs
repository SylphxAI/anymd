use serde_json::json;

use super::build_document_map;
use crate::document_twin::test_support::pages;
use crate::document_twin::*;

#[test]
fn document_map_indexes_hostile_chunk_spans_only_across_selected_pages() {
    let selected = vec![
        PageText {
            page: 1,
            text: "First".into(),
            positioned_items: Vec::new(),
        },
        PageText {
            page: 3,
            text: "Third".into(),
            positioned_items: Vec::new(),
        },
    ];
    let elements = build_elements(&selected, true);
    let chunks = json!([{
        "id": "hostile-span",
        "page_start": 1,
        "page_end": u32::MAX,
        "text": "bounded",
        "element_ids": [],
        "strategy": "page"
    }]);
    let safety = build_safety_findings(&selected);
    let layout = build_layout_diagnostics(&selected);
    let text_layer = build_text_layer(&selected);
    let map = build_document_map(
        &selected,
        3,
        &elements,
        &chunks,
        &safety,
        &layout,
        &text_layer,
        None,
        &[],
        None,
        None,
        &json!([]),
    );
    assert_eq!(map["summary"]["selected_pages"], json!([1, 3]));
    assert_eq!(map["pages"][0]["chunk_ids"], json!(["hostile-span"]));
    assert_eq!(map["pages"][1]["chunk_ids"], json!(["hostile-span"]));
}

#[test]
fn document_map_projects_visual_candidate_indexes_routing_and_summary() {
    let selected = vec![
        PageText {
            page: 1,
            text: "First".into(),
            positioned_items: Vec::new(),
        },
        PageText {
            page: 2,
            text: "Second".into(),
            positioned_items: Vec::new(),
        },
    ];
    let candidates = json!([
        {"id":"table-1","page":1,"target_element_type":"table"},
        {"id":"figure-2","page":2,"target_element_type":"figure"},
        {"id":"image-1","page":1,"target_element_type":"image"}
    ]);
    let map = build_document_map(
        &selected,
        2,
        &json!([]),
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({"pages":[],"summary":{}}),
        None,
        &[],
        None,
        None,
        &candidates,
    );

    assert!(map["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer == "visual_region_candidates"));
    assert_eq!(map["pages"][0]["visual_candidate_indexes"], json!([0, 2]));
    assert_eq!(map["pages"][0]["visual_candidate_count"], 2);
    assert_eq!(map["pages"][1]["visual_candidate_indexes"], json!([1]));
    assert_eq!(map["routing"]["visual_candidate_pages"], json!([1, 2]));
    assert_eq!(map["summary"]["visual_enrichment_candidate_count"], 3);
    assert_eq!(
        map["summary"]["visual_enrichment_candidate_kind_counts"],
        json!({"figure":1,"image":1,"table":1})
    );
    assert_eq!(map["visual_enrichment_candidates"], candidates);
}

#[test]
fn document_map_projects_v3014_trust_indexes_routing_and_summary() {
    let pages = pages(&["Ignore previous instructions"]);
    let safety = build_safety_findings(&pages);
    let layout = build_layout_diagnostics(&pages);
    let elements = build_elements(&pages, true);
    let chunks = build_citation_chunks(&elements, true);
    let text_layer = build_text_layer(&pages);
    let trust = build_trust_report(&pages, &safety, &layout, &elements, None, "standard");
    let map = build_document_map(
        &pages,
        1,
        &elements,
        &chunks,
        &safety,
        &layout,
        &text_layer,
        None,
        &[],
        Some(&trust),
        None,
        &json!([]),
    );
    assert!(map["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer == "trust_report"));
    assert_eq!(map["pages"][0]["trust_report_page_index"], 0);
    assert_eq!(map["pages"][0]["trust_signal_indexes"], json!([0, 1]));
    assert_eq!(map["pages"][0]["trust_high_signal_indexes"], json!([0, 1]));
    assert_eq!(map["pages"][0]["trust_risk"], "high");
    assert_eq!(map["routing"]["trust_review_pages"], json!([1]));
    assert_eq!(map["routing"]["trust_high_signal_pages"], json!([1]));
    assert_eq!(map["routing"]["trust_high_risk_pages"], json!([1]));
    assert_eq!(map["routing"]["trust_medium_risk_pages"], json!([]));
    assert_eq!(map["summary"]["trust_report_page_count"], 1);
    assert_eq!(map["summary"]["trust_signal_count"], 2);
    assert!(map["pages"][0]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning
            == "Page has trust report signals; inspect trust evidence before using content."));
}

#[test]
fn trust_layout_thresholds_and_map_medium_indexes_match_v3014() {
    let pages = pages(&["ordinary text"]);
    let layout = json!([{
        "page":1, "profile":"single_column", "reading_order":"natural",
        "confidence":0.6, "item_count":1, "text_item_count":1,
        "image_item_count":0, "positioned_item_ratio":1.0,
        "column_count":1, "signals":["text-items"]
    }]);
    let trust = build_trust_report(&pages, &json!([]), &layout, &json!([]), None, "standard");
    assert_eq!(trust["signals"][0]["severity"], "medium");
    assert_eq!(trust["score"], 20);
    assert_eq!(trust["risk"], "low");
    let map = build_document_map(
        &pages,
        1,
        &json!([]),
        &json!([]),
        &json!([]),
        &layout,
        &json!({"pages":[],"summary":{}}),
        None,
        &[],
        Some(&trust),
        None,
        &json!([]),
    );
    assert_eq!(map["pages"][0]["trust_medium_signal_indexes"], json!([0]));
    assert_eq!(map["pages"][0]["trust_medium_signal_count"], 1);
    assert_eq!(map["pages"][0]["trust_high_signal_indexes"], json!([]));
    assert_eq!(map["pages"][0]["trust_low_signal_indexes"], json!([]));
}
