use super::*;
use serde_json::json;

use crate::document_twin::PageText;
use crate::text_index::{PositionedTextItem, TextBoundingBox};

#[test]
fn semantic_hints_match_ts_precedence_and_complete_metadata() {
    let bounds = PageSemanticBounds {
        left: 0.0,
        right: 612.0,
        bottom: 0.0,
        top: 792.0,
    };
    let stats = PageSemanticStats {
        max_height: 18.0,
        median_height: 10.0,
        text_item_count: 8,
        bounds: Some(bounds),
    };
    let box_at = |left, bottom, right, top| TextBoundingBox {
        left,
        bottom,
        right,
        top,
    };
    let value = |text, box_| serde_json::to_value(semantic_hint(text, box_, stats)).unwrap();

    assert_eq!(
        value(
            "Equation (1): Loss function",
            Some(box_at(10.0, 770.0, 220.0, 780.0))
        ),
        json!({"role":"caption","confidence":0.86,"signals":["caption-prefix"]})
    );
    assert_eq!(
        value(
            "Confidential Report",
            Some(box_at(10.0, 770.0, 160.0, 780.0))
        ),
        json!({"role":"header","confidence":0.82,"signals":["page-top-band","compact-edge-text","header-pattern"]})
    );
    assert_eq!(
        value("Page 1 of 2", Some(box_at(10.0, 10.0, 90.0, 20.0))),
        json!({"role":"footer","confidence":0.88,"signals":["page-bottom-band","compact-edge-text","footer-pattern"]})
    );
    assert_eq!(
        value("Chapter 1: Intro", Some(box_at(10.0, 500.0, 150.0, 512.0))),
        json!({"role":"heading","confidence":0.84,"signals":["section-heading-pattern","named-section-prefix","short-line"],"level":1})
    );
    assert_eq!(
        value("1.2 Scope", Some(box_at(10.0, 480.0, 100.0, 492.0))),
        json!({"role":"heading","confidence":0.84,"signals":["section-heading-pattern","numbered-section-prefix","short-line"],"level":2})
    );
    assert_eq!(
        value("IV. Results", Some(box_at(10.0, 460.0, 100.0, 472.0))),
        json!({"role":"heading","confidence":0.82,"signals":["section-heading-pattern","roman-section-prefix","short-line"],"level":1})
    );
    assert_eq!(
        value("[x] item", None),
        json!({"role":"list_item","confidence":0.92,"signals":["list-prefix"]})
    );
    assert_eq!(
        value("Ordinary sentence.", None),
        json!({"role":"paragraph","confidence":0.5,"signals":["default-text"]})
    );
}

#[test]
fn semantic_hints_use_utf16_thresholds_and_valid_page_bounds() {
    let stats = PageSemanticStats {
        max_height: 10.0,
        median_height: 10.0,
        text_item_count: 2,
        bounds: None,
    };
    let title_120 = format!("A{}😀", "x".repeat(117));
    let title_121 = format!("A{}😀", "x".repeat(118));
    assert_eq!(title_120.encode_utf16().count(), 120);
    assert_eq!(title_121.encode_utf16().count(), 121);
    assert_eq!(
        semantic_hint(&format!("1 {title_120}"), None, stats).role,
        "heading"
    );
    assert_eq!(
        semantic_hint(&format!("1 {title_121}"), None, stats).role,
        "paragraph"
    );

    let geometry = json!([
        {"page":1,"view_box":{"left":0,"bottom":0,"right":612,"top":792}},
        {"page":2,"view_box":{"left":0,"bottom":0,"right":0,"top":792}},
        {"page":3,"view_box":{"left":0,"bottom":0,"right":"bad","top":792}}
    ]);
    let parsed = page_semantic_bounds(Some(&geometry));
    assert!(parsed.contains_key(&1));
    assert!(!parsed.contains_key(&2));
    assert!(!parsed.contains_key(&3));

    let edge_box = TextBoundingBox {
        left: -20.0,
        bottom: 770.0,
        right: 100.0,
        top: 780.0,
    };
    let with_bounds = PageSemanticStats {
        bounds: parsed.get(&1).copied(),
        ..stats
    };
    assert_eq!(
        semantic_hint("Confidential Report", Some(edge_box), with_bounds).role,
        "paragraph"
    );
}

#[test]
fn semantic_height_stats_handle_even_odd_and_admitted_maximum_linearly() {
    let make_item = |height: f64| PositionedTextItem {
        text: "x".into(),
        bounding_box: Some(TextBoundingBox {
            left: 0.0,
            bottom: 0.0,
            right: 1.0,
            top: height,
        }),
        chars: Vec::new(),
        runs: Vec::new(),
    };
    let even = PageText {
        page: 1,
        text: String::new(),
        positioned_items: vec![make_item(8.0), make_item(12.0)],
    };
    assert_eq!(page_semantic_stats(&even, None).median_height, 10.0);
    let odd = PageText {
        page: 1,
        text: String::new(),
        positioned_items: vec![make_item(8.0), make_item(12.0), make_item(18.0)],
    };
    assert_eq!(page_semantic_stats(&odd, None).median_height, 12.0);

    let admitted = PageText {
        page: u32::MAX,
        text: String::new(),
        positioned_items: (0..250_000).map(|_| make_item(10.0)).collect(),
    };
    let admitted_stats = page_semantic_stats(&admitted, None);
    assert_eq!(admitted_stats.text_item_count, 250_000);
    assert_eq!(admitted_stats.median_height, 10.0);
    assert_eq!(admitted_stats.max_height, 10.0);
}
