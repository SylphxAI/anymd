//! Selectable text layer: lines, words, and runs with UTF-16 offsets and geometry.

use serde_json::{json, Value};

use super::PageText;
use crate::text_index::{PositionedTextItem, PositionedTextRun, TextBoundingBox};

pub(super) fn merge_text_boxes(
    current: Option<TextBoundingBox>,
    next: TextBoundingBox,
) -> Option<TextBoundingBox> {
    let merged = current.map_or(next, |current| TextBoundingBox {
        left: current.left.min(next.left),
        bottom: current.bottom.min(next.bottom),
        right: current.right.max(next.right),
        top: current.top.max(next.top),
    });
    let width = merged.right - merged.left;
    let height = merged.top - merged.bottom;
    [
        merged.left,
        merged.bottom,
        merged.right,
        merged.top,
        width,
        height,
    ]
    .into_iter()
    .all(f64::is_finite)
    .then_some(merged)
}

pub(super) fn utf16_len(value: &str) -> u32 {
    value.encode_utf16().count().try_into().unwrap_or(u32::MAX)
}

fn item_words(item: &PositionedTextItem, line_start: u32) -> Vec<Value> {
    let mut words = Vec::new();
    let mut word_start: Option<usize> = None;
    let mut boundaries = item.text.char_indices().collect::<Vec<_>>();
    boundaries.push((item.text.len(), '\0'));
    for (byte_index, character) in boundaries {
        let is_boundary = byte_index == item.text.len() || character.is_whitespace();
        if !is_boundary && word_start.is_none() {
            word_start = Some(byte_index);
        }
        let Some(start) = word_start else {
            continue;
        };
        if !is_boundary {
            continue;
        }
        let text = &item.text[start..byte_index];
        let item_start = utf16_len(&item.text[..start]);
        let item_end = item_start.saturating_add(utf16_len(text));
        let bounding_box = item
            .chars
            .iter()
            .filter(|char_| {
                !char_.is_whitespace
                    && char_.item_char_start >= item_start
                    && char_.item_char_end <= item_end
            })
            .filter_map(|char_| char_.bounding_box)
            .try_fold(None, |current, next| {
                merge_text_boxes(current, next).map(Some).ok_or(())
            })
            .ok()
            .flatten();
        let mut word = json!({
            "index": words.len(),
            "text": text,
            "char_start": line_start.saturating_add(item_start),
            "char_end": line_start.saturating_add(item_end),
        });
        if let Some(box_) = bounding_box {
            word["bounding_box"] = json!(box_);
            word["bounding_box_level"] = json!("char_estimated");
            word["confidence"] = json!(0.74);
        }
        words.push(word);
        word_start = None;
    }
    words
}

/// Build the bounded selectable-text geometry projection. Coordinates are
/// source-derived item boxes with uniformly estimated UTF-16 character boxes,
/// not glyph outlines.
fn partition_text_runs(item: &PositionedTextItem) -> (Vec<PositionedTextRun>, Vec<(usize, usize)>) {
    let fallback = || {
        (
            vec![PositionedTextRun {
                text: item.text.clone(),
                item_char_start: 0,
                item_char_end: utf16_len(&item.text),
                bounding_box: item.bounding_box,
            }],
            vec![(0, item.chars.len())],
        )
    };
    if item.runs.is_empty() {
        return fallback();
    }

    let item_end = utf16_len(&item.text);
    let mut expected_start = 0u32;
    for run in &item.runs {
        let run_len = utf16_len(&run.text);
        if run.item_char_start != expected_start
            || run.item_char_end < run.item_char_start
            || run.item_char_end - run.item_char_start != run_len
            || run.item_char_end > item_end
        {
            return fallback();
        }
        expected_start = run.item_char_end;
    }
    if expected_start != item_end {
        return fallback();
    }

    let mut ranges = Vec::with_capacity(item.runs.len());
    let mut cursor = 0usize;
    let mut previous_char_end = 0u32;
    for run in &item.runs {
        let start = cursor;
        while let Some(character) = item.chars.get(cursor) {
            if character.item_char_start >= run.item_char_end {
                break;
            }
            if character.item_char_start < run.item_char_start
                || character.item_char_end < character.item_char_start
                || character.item_char_end > run.item_char_end
                || character.item_char_start < previous_char_end
            {
                return fallback();
            }
            previous_char_end = character.item_char_end;
            cursor += 1;
        }
        ranges.push((start, cursor));
    }
    if cursor != item.chars.len() {
        return fallback();
    }
    (item.runs.clone(), ranges)
}

pub fn build_text_layer(pages: &[PageText]) -> Value {
    let mut output_pages = Vec::new();
    let mut warnings = Vec::new();
    let mut run_count = 0usize;
    let mut line_count = 0usize;
    let mut word_count = 0usize;
    let mut char_count = 0u64;
    let mut chars_with_boxes = 0usize;
    let mut runs_with_boxes = 0usize;
    let mut lines_with_boxes = 0usize;
    let mut words_with_boxes = 0usize;

    for page in pages {
        let mut lines = Vec::new();
        let mut text_parts = Vec::new();
        let mut page_offset = 0u32;
        let fallback_items = if page.positioned_items.is_empty() {
            page.text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    let mut offset = 0u32;
                    let chars = line
                        .chars()
                        .map(|character| {
                            let text = character.to_string();
                            let start = offset;
                            offset = offset.saturating_add(utf16_len(&text));
                            crate::text_index::TextCharacterGeometry {
                                text,
                                item_char_start: start,
                                item_char_end: offset,
                                is_whitespace: character.is_whitespace(),
                                bounding_box: None,
                            }
                        })
                        .collect();
                    PositionedTextItem {
                        text: line.to_string(),
                        bounding_box: None,
                        chars,
                        runs: Vec::new(),
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let items = if page.positioned_items.is_empty() {
            &fallback_items
        } else {
            &page.positioned_items
        };
        for item in items.iter().filter(|item| !item.text.trim().is_empty()) {
            if !text_parts.is_empty() {
                text_parts.push("\n".to_string());
                page_offset = page_offset.saturating_add(1);
            }
            let line_start = page_offset;
            let line_end = line_start.saturating_add(utf16_len(&item.text));
            let (source_runs, run_char_ranges) = partition_text_runs(item);
            let mut chars = Vec::new();
            let mut runs = Vec::with_capacity(source_runs.len());
            for (run_index, (source_run, (char_start, char_end))) in
                source_runs.iter().zip(run_char_ranges).enumerate()
            {
                let run_chars = item.chars[char_start..char_end]
                    .iter()
                    .enumerate()
                    .map(|(index, char_)| {
                        let mut value = json!({
                            "index": index,
                            "text": char_.text,
                            "char_start": line_start.saturating_add(char_.item_char_start),
                            "char_end": line_start.saturating_add(char_.item_char_end),
                            "run_index": run_index,
                            "is_whitespace": char_.is_whitespace,
                        });
                        if let Some(box_) = char_.bounding_box {
                            value["bounding_box"] = json!(box_);
                            value["bounding_box_level"] = json!("char_estimated");
                            value["confidence"] = json!(0.74);
                        }
                        value
                    })
                    .collect::<Vec<_>>();
                let run_has_boxes = run_chars
                    .iter()
                    .any(|char_| char_.get("bounding_box").is_some());
                chars.extend(run_chars.iter().cloned());
                let mut run = json!({
                    "index": run_index,
                    "text": source_run.text,
                    "char_start": line_start.saturating_add(source_run.item_char_start),
                    "char_end": line_start.saturating_add(source_run.item_char_end),
                    "chars": run_chars,
                    "provenance": {
                        "engine": "pdf-reader-core",
                        "source": "selectable-text",
                        "bounding_box_level": if run_has_boxes { "char_estimated" } else { "text_run" },
                    },
                });
                if let Some(box_) = source_run.bounding_box {
                    run["bounding_box"] = json!(box_);
                    runs_with_boxes += 1;
                }
                runs.push(run);
            }
            let words = item_words(item, line_start);
            let mut line = json!({
                "id": format!("p{}-line-{}", page.page, lines.len() + 1),
                "index": lines.len(),
                "text": item.text,
                "char_start": line_start,
                "char_end": line_end,
                "runs": runs,
                "words": words,
                "chars": chars,
                "provenance": {
                    "engine": "pdf-reader-core",
                    "source": "selectable-text",
                    "bounding_box_level": if item.chars.iter().any(|char_| char_.bounding_box.is_some()) { "char_estimated" } else { "line" },
                },
            });
            if let Some(box_) = item.bounding_box {
                line["bounding_box"] = json!(box_);
                lines_with_boxes += 1;
            } else {
                warnings.push(format!(
                    "Page {} line {} has no bounding box.",
                    page.page,
                    lines.len()
                ));
            }
            chars_with_boxes += item
                .chars
                .iter()
                .filter(|char_| char_.bounding_box.is_some())
                .count();
            words_with_boxes += line["words"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|word| word.get("bounding_box").is_some())
                .count();
            word_count += line["words"].as_array().map_or(0, Vec::len);
            run_count += source_runs.len();
            line_count += 1;
            text_parts.push(item.text.clone());
            page_offset = line_end;
            lines.push(line);
        }
        let text = text_parts.concat();
        char_count = char_count.saturating_add(u64::from(utf16_len(&text)));
        output_pages.push(json!({
            "page": page.page,
            "text": text,
            "char_count": utf16_len(&text),
            "line_count": lines.len(),
            "word_count": lines.iter().map(|line| line["words"].as_array().map_or(0, Vec::len)).sum::<usize>(),
            "lines": lines,
        }));
    }

    let mut layer = json!({
        "version": "2026-06-15",
        "profile": "pdf_text_layer",
        "pages": output_pages,
        "summary": {
            "selected_pages": pages.iter().map(|page| page.page).collect::<Vec<_>>(),
            "page_count": pages.len(),
            "run_count": run_count,
            "line_count": line_count,
            "word_count": word_count,
            "char_count": char_count,
            "chars_with_bounding_boxes": chars_with_boxes,
            "runs_with_bounding_boxes": runs_with_boxes,
            "lines_with_bounding_boxes": lines_with_boxes,
            "words_with_bounding_boxes": words_with_boxes,
            "runs_with_font_metadata": 0,
            "runs_with_direction_metadata": 0,
            "runs_with_transform_metadata": 0,
            "runs_with_eol_metadata": 0,
        },
    });
    if !warnings.is_empty() {
        layer["warnings"] = json!(warnings);
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_twin::*;

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
}
