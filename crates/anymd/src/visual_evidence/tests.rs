use std::path::PathBuf;

use serde_json::{json, Value};

use crate::page_selection::{selected_pages, MAX_SELECTED_PAGES};
use crate::schema::PageSpecifier;

use super::regions::region_warnings;
use super::{
    extract_regions, render_pages, render_warnings, RequestWorkBudget, MAX_REQUEST_REGIONS,
    MAX_REQUEST_RENDERED_PAGES, MAX_REQUEST_RENDER_PIXELS, MAX_SOURCES_PER_REQUEST,
};

fn fixture_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-behavior-v1.pdf")
        .canonicalize()
        .expect("canonical fixture")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn page_selection_matches_ts_sort_dedupe_and_prefix_parse() {
    assert_eq!(
        selected_pages(&Some(PageSpecifier::Range("3,1,1,2x".into()))).unwrap(),
        Some(vec![1, 2, 3])
    );
    assert_eq!(
        selected_pages(&Some(PageSpecifier::Range("2-4".into()))).unwrap(),
        Some(vec![2, 3, 4])
    );
    for open in ["1--5", "1-   "] {
        let pages = selected_pages(&Some(PageSpecifier::Range(open.into())))
            .unwrap()
            .unwrap();
        assert_eq!(pages.len(), MAX_SELECTED_PAGES);
        assert_eq!(pages.first(), Some(&1));
        assert_eq!(pages.last(), Some(&10_001));
    }
    assert_eq!(
        selected_pages(&Some(PageSpecifier::Range("1-2-999".into()))).unwrap(),
        Some(vec![1, 2])
    );
}

#[test]
fn warning_text_matches_v3014_contract() {
    assert_eq!(
        render_warnings(&[9], &[3, 4], 2, 2),
        vec![
            "Requested pages 9 exceed document page count 2.",
            "Rendered first 2 selected pages; skipped 3, 4 due to max_pages.",
        ]
    );
    assert_eq!(
        region_warnings(&[9, 9], 2, 3, 1),
        vec![
            "Requested region pages 9 exceed document page count 3.",
            "Cropped first 1 valid regions; skipped 2 due to max_regions.",
        ]
    );
}

#[test]
fn public_render_envelope_has_raw_image_part_and_no_inline_data() {
    let result = render_pages(json!({
        "operation": "render_page",
        "sources": [{"path": fixture_path(), "pages": [1]}],
        "scale": 1,
        "max_pages": 1,
    }))
    .expect("render result");
    let encoded = serde_json::to_value(result).expect("serialize result");
    assert_eq!(encoded["isError"], false);
    assert_eq!(encoded["content"][0]["type"], "text");
    assert_eq!(encoded["content"][1]["type"], "image");
    assert_eq!(encoded["content"][1]["mimeType"], "image/png");
    let image = encoded["content"][1]["data"].as_str().expect("image data");
    assert!(!image.starts_with("data:"));
    let payload: Value = serde_json::from_str(
        encoded["content"][0]["text"]
            .as_str()
            .expect("text payload"),
    )
    .expect("parse text payload");
    assert_eq!(payload["profile"], "page_render_evidence");
    assert_eq!(payload["results"][0]["rendered_pages"][0]["page"], 1);
    assert_eq!(
        payload["results"][0]["rendered_pages"][0]["image_content_index"],
        1
    );
    assert!(payload["results"][0]["rendered_pages"][0]
        .get("data")
        .is_none());
}

#[test]
fn render_selection_and_region_group_order_match_v3014() {
    let render = render_pages(json!({
        "operation": "render_page",
        "sources": [{"path": fixture_path(), "pages": [2, 99, 1, 2]}],
        "scale": 1,
        "max_pages": 1,
        "include_image": false,
    }))
    .expect("render result");
    let payload = render.structured_content.expect("structured render");
    assert_eq!(payload["results"][0]["rendered_pages"][0]["page"], 1);
    assert_eq!(
        payload["results"][0]["warnings"],
        json!([
            "Requested pages 99 exceed document page count 3.",
            "Rendered first 1 selected pages; skipped 2 due to max_pages."
        ])
    );
    assert_eq!(render.content.len(), 1);
    assert!(payload["results"][0]["rendered_pages"][0]
        .get("image_content_index")
        .is_none());

    let crops = extract_regions(json!({
        "operation": "extract_regions",
        "sources": [{
            "path": fixture_path(),
            "regions": [
                {"id": "A", "page": 2, "bounding_box": {"left": 0, "bottom": 0, "right": 20, "top": 20}},
                {"page": 1, "bounding_box": {"left": 0, "bottom": 0, "right": 20, "top": 20}},
                {"id": "C", "page": 2, "bounding_box": {"left": 20, "bottom": 20, "right": 40, "top": 40}}
            ]
        }],
        "scale": 1,
        "include_image": false,
    }))
    .expect("crop result");
    let payload = crops.structured_content.expect("structured crops");
    let ids = payload["results"][0]["regions"]
        .as_array()
        .expect("regions")
        .iter()
        .map(|region| region["region_id"].as_str().expect("region id"))
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["A", "C", "region-2"]);
    assert_eq!(crops.content.len(), 1);
}

#[test]
fn late_source_failure_discards_staged_images_and_preserves_global_indexes() {
    let path = fixture_path();
    let crops = extract_regions(json!({
        "operation": "extract_regions",
        "sources": [
            {
                "path": path,
                "regions": [
                    {"id": "staged", "page": 1, "bounding_box": {"left": 0, "bottom": 0, "right": 20, "top": 20}},
                    {"id": "late-failure", "page": 1, "bounding_box": {"left": 10, "bottom": 10, "right": 10, "top": 20}}
                ]
            },
            {
                "path": fixture_path(),
                "regions": [
                    {"id": "survivor", "page": 1, "bounding_box": {"left": 0, "bottom": 0, "right": 20, "top": 20}}
                ]
            }
        ],
        "scale": 1,
    }))
    .expect("mixed crop result");
    let payload = crops.structured_content.expect("structured crops");
    assert_eq!(payload["results"][0]["success"], false);
    assert_eq!(payload["results"][1]["success"], true);
    assert_eq!(
        payload["results"][1]["regions"][0]["image_content_index"],
        1
    );
    assert_eq!(crops.content.len(), 2);
}

#[test]
fn request_work_budget_is_charged_before_work_and_never_rolls_back() {
    let mut budget = RequestWorkBudget::default();
    for _ in 0..MAX_REQUEST_RENDERED_PAGES {
        budget.charge_page(1).expect("page within bound");
    }
    assert!(budget.charge_page(1).unwrap_err().contains("pages"));
    assert_eq!(budget.rendered_pages, MAX_REQUEST_RENDERED_PAGES);
    assert!(budget
        .ensure_available()
        .expect_err("page exhaustion is sticky")
        .contains("pages"));

    let mut pixel_budget = RequestWorkBudget::default();
    pixel_budget
        .charge_page(MAX_REQUEST_RENDER_PIXELS)
        .expect("pixels at bound");
    assert!(pixel_budget.charge_page(1).unwrap_err().contains("pixels"));
    assert_eq!(pixel_budget.render_pixels, MAX_REQUEST_RENDER_PIXELS);
    assert!(pixel_budget
        .ensure_available()
        .expect_err("pixel exhaustion is sticky")
        .contains("pixels"));

    let mut region_budget = RequestWorkBudget::default();
    for _ in 0..MAX_REQUEST_REGIONS {
        region_budget.charge_region().expect("region within bound");
    }
    assert!(region_budget
        .charge_region()
        .unwrap_err()
        .contains("regions"));
    assert_eq!(region_budget.regions, MAX_REQUEST_REGIONS);
    assert!(region_budget
        .ensure_available()
        .expect_err("region exhaustion is sticky")
        .contains("regions"));
}

#[test]
fn source_count_is_rejected_before_any_io_or_render_work() {
    let sources = (0..=MAX_SOURCES_PER_REQUEST)
        .map(|index| json!({"path": format!("missing-{index}.pdf")}))
        .collect::<Vec<_>>();
    let error = render_pages(json!({
        "operation": "render_page",
        "sources": sources,
    }))
    .expect_err("too many sources");
    assert!(error.message.contains("at most 32 sources"));
}
