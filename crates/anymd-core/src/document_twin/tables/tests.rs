use serde_json::json;

use super::grid::MAX_TABLE_ITEMS_PER_PAGE;
use super::{build_tables, build_tables_with_admission, PageText};
use crate::document_twin::text_layer::utf16_len;
use crate::text_index::{PositionedTextItem, PositionedTextRun, TextBoundingBox};

#[test]
fn detects_simple_table() {
    let item = |text: &str, left: f64, bottom: f64| PositionedTextItem {
        text: text.into(),
        bounding_box: Some(TextBoundingBox {
            left,
            bottom,
            right: left + 30.0,
            top: bottom + 10.0,
        }),
        chars: Vec::new(),
        runs: Vec::new(),
    };
    let pages = vec![PageText {
        page: 1,
        text: String::new(),
        positioned_items: vec![
            item("Name", 10.0, 100.0),
            item("Qty", 100.0, 100.0),
            item("Price", 200.0, 100.0),
            item("Apple", 10.0, 80.0),
            item("2", 100.0, 80.0),
            item("1.50", 200.0, 80.0),
            item("Pear", 10.0, 60.0),
            item("3", 100.0, 60.0),
            item("2.00", 200.0, 60.0),
        ],
    }];
    let tables = build_tables(&pages);
    let arr = tables.as_array().unwrap();
    assert!(!arr.is_empty());
    assert_eq!(arr[0]["rowCount"], 3);
    assert_eq!(arr[0]["colCount"], 3);
}

#[test]
fn selectable_tables_consume_preserved_source_runs_after_text_segmentation() {
    let run = |text: &str, start: u32, left: f64, bottom: f64| PositionedTextRun {
        text: text.into(),
        item_char_start: start,
        item_char_end: start + utf16_len(text),
        bounding_box: Some(TextBoundingBox {
            left,
            bottom,
            right: left + 30.0,
            top: bottom + 10.0,
        }),
    };
    let pages = [PageText {
        page: 1,
        text: "NameQtyApple2".into(),
        positioned_items: vec![PositionedTextItem {
            text: "NameQtyApple2".into(),
            bounding_box: Some(TextBoundingBox {
                left: 20.0,
                bottom: 80.0,
                right: 150.0,
                top: 110.0,
            }),
            chars: Vec::new(),
            runs: vec![
                run("Name", 0, 20.0, 100.0),
                run("Qty", 4, 120.0, 100.0),
                run("Apple", 7, 20.0, 80.0),
                run("2", 12, 120.0, 80.0),
            ],
        }],
    }];

    let tables = build_tables(&pages);
    assert_eq!(tables[0]["rows"], json!([["Name", "Qty"], ["Apple", "2"]]));
    assert_eq!(tables[0]["rowCount"], 2);
    assert_eq!(tables[0]["colCount"], 2);
}

#[test]
fn selectable_tables_reset_page_indexes_and_link_repeated_headers() {
    let item = |text: &str, left: f64, bottom: f64| PositionedTextItem {
        text: text.into(),
        bounding_box: Some(TextBoundingBox {
            left,
            bottom,
            right: left + 30.0,
            top: bottom + 10.0,
        }),
        chars: Vec::new(),
        runs: Vec::new(),
    };
    let pages = vec![
        PageText {
            page: 1,
            text: String::new(),
            positioned_items: vec![
                item("Name", 20.0, 100.0),
                item("Qty", 120.0, 100.0),
                item("Apple", 20.0, 80.0),
                item("2", 120.0, 80.0),
            ],
        },
        PageText {
            page: 2,
            text: String::new(),
            positioned_items: vec![
                item("name", 20.0, 700.0),
                item("QTY", 120.0, 700.0),
                item("Pear", 20.0, 680.0),
                item("3", 120.0, 680.0),
            ],
        },
    ];
    let tables = build_tables(&pages);
    assert_eq!(tables.as_array().unwrap().len(), 2);
    assert_eq!(tables[0]["tableIndex"], 0);
    assert_eq!(tables[1]["tableIndex"], 0);
    assert_eq!(tables[0]["continuation"]["role"], "starts");
    assert_eq!(tables[0]["continuation"]["nextTableId"], "p2-table-1");
    assert_eq!(tables[0]["continuation"]["confidence"], 0.95);
    assert_eq!(tables[1]["continuation"]["role"], "ends");
    assert!(tables[0]["quality"]["signals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|signal| signal == "multi_page_continuation_candidate"));
}

#[test]
fn selectable_tables_match_direct_and_page_content_geometry_paths() {
    let item = |text: &str, left: f64, bottom: f64| PositionedTextItem {
        text: text.into(),
        bounding_box: Some(TextBoundingBox {
            left,
            bottom,
            right: left + 30.0,
            top: bottom + 10.0,
        }),
        chars: Vec::new(),
        runs: Vec::new(),
    };
    let pages = [PageText {
        page: 1,
        text: String::new(),
        positioned_items: vec![
            item("Name", 20.0, 100.0),
            item("Qty", 120.0, 100.0),
            item("Apple", 20.0, 80.0),
            item("2", 120.0, 80.0),
        ],
    }];
    let direct = build_tables_with_admission(&pages, false).0;
    let page_content = build_tables_with_admission(&pages, true).0;
    assert_eq!(direct[0]["quality"]["mergedCellCandidateCount"], 0);
    assert_eq!(direct[0]["cells"][0]["colSpan"], 1);
    assert_eq!(page_content[0]["quality"]["mergedCellCandidateCount"], 2);
    assert_eq!(page_content[0]["cells"][0]["colSpan"], 2);
    assert_eq!(
        page_content[0]["quality"]["warnings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn selectable_table_admission_accepts_the_exact_item_cap() {
    let positioned_items = (0..MAX_TABLE_ITEMS_PER_PAGE)
        .map(|index| PositionedTextItem {
            text: format!("cell-{index}"),
            bounding_box: Some(TextBoundingBox {
                left: if index % 2 == 0 { 10.0 } else { 100.0 },
                bottom: 30_000.0 - (index / 2) as f64 * 10.0,
                right: if index % 2 == 0 { 30.0 } else { 120.0 },
                top: 30_010.0 - (index / 2) as f64 * 10.0,
            }),
            chars: Vec::new(),
            runs: Vec::new(),
        })
        .collect();
    let (tables, warnings) = build_tables_with_admission(
        &[PageText {
            page: 6,
            text: String::new(),
            positioned_items,
        }],
        false,
    );
    assert!(warnings.is_empty());
    assert_eq!(tables[0]["rowCount"], 2_048);
    assert_eq!(tables[0]["colCount"], 2);
}

#[test]
fn selectable_table_admission_fails_closed_before_grid_amplification() {
    let positioned_items = (0..=MAX_TABLE_ITEMS_PER_PAGE)
        .map(|index| PositionedTextItem {
            text: format!("cell-{index}"),
            bounding_box: Some(TextBoundingBox {
                left: if index % 2 == 0 { 10.0 } else { 100.0 },
                bottom: 700.0 - (index / 2) as f64,
                right: if index % 2 == 0 { 30.0 } else { 120.0 },
                top: 710.0 - (index / 2) as f64,
            }),
            chars: Vec::new(),
            runs: Vec::new(),
        })
        .collect();
    let (tables, warnings) = build_tables_with_admission(
        &[PageText {
            page: 7,
            text: String::new(),
            positioned_items,
        }],
        false,
    );
    assert_eq!(tables, json!([]));
    assert_eq!(
        warnings,
        vec!["Selectable table extraction skipped page 7: spatial grid exceeds the Rust admission limit."]
    );
}
