use serde_json::json;

use super::{build_text_layer, runs::partition_text_runs};
use crate::document_twin::{build_elements, PageText};
use crate::text_index::{PositionedTextItem, PositionedTextRun, TextBoundingBox};

#[test]
fn selectable_text_layer_and_elements_share_utf16_geometry() {
    let line_box = TextBoundingBox {
        left: 0.0,
        bottom: 10.0,
        right: 50.0,
        top: 22.0,
    };
    let char_ = |text: &str, start, end, left, right, whitespace| {
        crate::text_index::TextCharacterGeometry {
            text: text.into(),
            item_char_start: start,
            item_char_end: end,
            is_whitespace: whitespace,
            bounding_box: Some(TextBoundingBox {
                left,
                bottom: 10.0,
                right,
                top: 22.0,
            }),
        }
    };
    let pages = vec![PageText {
        page: 3,
        text: "A😀 B".into(),
        positioned_items: vec![PositionedTextItem {
            text: "A😀 B".into(),
            bounding_box: Some(line_box),
            chars: vec![
                char_("A", 0, 1, 0.0, 10.0, false),
                char_("😀", 1, 3, 10.0, 30.0, false),
                char_(" ", 3, 4, 30.0, 40.0, true),
                char_("B", 4, 5, 40.0, 50.0, false),
            ],
            runs: Vec::new(),
        }],
    }];

    let layer = build_text_layer(&pages);
    assert_eq!(layer["version"], "2026-06-15");
    assert_eq!(layer["pages"][0]["char_count"], 5);
    assert_eq!(layer["pages"][0]["word_count"], 2);
    assert_eq!(layer["pages"][0]["lines"][0]["chars"][1]["char_start"], 1);
    assert_eq!(layer["pages"][0]["lines"][0]["chars"][1]["char_end"], 3);
    assert_eq!(layer["pages"][0]["lines"][0]["words"][1]["char_start"], 4);
    assert_eq!(layer["summary"]["chars_with_bounding_boxes"], 4);
    assert_eq!(layer["summary"]["words_with_bounding_boxes"], 2);

    let elements = build_elements(&pages, false);
    assert_eq!(elements[0]["id"], "p3-text-1");
    assert_eq!(elements[0]["bounding_box"], json!(line_box));
    assert_eq!(elements[0]["provenance"]["engine"], "pdf-reader-core");
}

#[test]
fn selectable_text_layer_preserves_multipart_run_boundaries() {
    let first_box = TextBoundingBox {
        left: 0.0,
        bottom: 10.0,
        right: 20.0,
        top: 22.0,
    };
    let second_box = TextBoundingBox {
        left: 60.0,
        bottom: 10.0,
        right: 70.0,
        top: 22.0,
    };
    let pages = vec![PageText {
        page: 1,
        text: "A😀B".into(),
        positioned_items: vec![PositionedTextItem {
            text: "A😀B".into(),
            bounding_box: Some(TextBoundingBox {
                left: first_box.left,
                bottom: first_box.bottom,
                right: second_box.right,
                top: first_box.top,
            }),
            chars: vec![
                crate::text_index::TextCharacterGeometry {
                    text: "A".into(),
                    item_char_start: 0,
                    item_char_end: 1,
                    is_whitespace: false,
                    bounding_box: Some(first_box),
                },
                crate::text_index::TextCharacterGeometry {
                    text: "😀".into(),
                    item_char_start: 1,
                    item_char_end: 3,
                    is_whitespace: false,
                    bounding_box: Some(first_box),
                },
                crate::text_index::TextCharacterGeometry {
                    text: "B".into(),
                    item_char_start: 3,
                    item_char_end: 4,
                    is_whitespace: false,
                    bounding_box: Some(second_box),
                },
            ],
            runs: vec![
                PositionedTextRun {
                    text: "A😀".into(),
                    item_char_start: 0,
                    item_char_end: 3,
                    bounding_box: Some(first_box),
                },
                PositionedTextRun {
                    text: "B".into(),
                    item_char_start: 3,
                    item_char_end: 4,
                    bounding_box: Some(second_box),
                },
            ],
        }],
    }];

    let layer = build_text_layer(&pages);
    let runs = layer["pages"][0]["lines"][0]["runs"]
        .as_array()
        .expect("runs");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["text"], "A😀");
    assert_eq!(runs[0]["char_end"], 3);
    assert_eq!(runs[0]["chars"][1]["char_end"], 3);
    assert_eq!(runs[1]["text"], "B");
    assert_eq!(runs[1]["char_start"], 3);
    assert_eq!(runs[1]["chars"][0]["run_index"], 1);
    assert_eq!(layer["summary"]["run_count"], 2);
    assert_eq!(layer["summary"]["runs_with_bounding_boxes"], 2);
}

#[test]
fn text_layer_partitions_many_runs_linearly_and_falls_back_on_malformed_ranges() {
    let run_count = 4_096usize;
    let item = PositionedTextItem {
        text: "x".repeat(run_count),
        bounding_box: None,
        chars: (0..run_count)
            .map(|index| crate::text_index::TextCharacterGeometry {
                text: "x".into(),
                item_char_start: index as u32,
                item_char_end: index as u32 + 1,
                is_whitespace: false,
                bounding_box: None,
            })
            .collect(),
        runs: (0..run_count)
            .map(|index| PositionedTextRun {
                text: "x".into(),
                item_char_start: index as u32,
                item_char_end: index as u32 + 1,
                bounding_box: None,
            })
            .collect(),
    };
    let (runs, ranges) = partition_text_runs(&item);
    assert_eq!(runs.len(), run_count);
    assert_eq!(ranges.len(), run_count);
    assert_eq!(ranges[0], (0, 1));
    assert_eq!(ranges[run_count - 1], (run_count - 1, run_count));

    let mut malformed = item;
    malformed.runs[2].item_char_start = 1;
    let (runs, ranges) = partition_text_runs(&malformed);
    assert_eq!(runs.len(), 1);
    assert_eq!(ranges, vec![(0, run_count)]);
}
