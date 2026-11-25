//! Project validated source runs and their characters into page-relative records.

use serde_json::{json, Value};

use super::utf16_len;
use crate::text_index::{PositionedTextItem, PositionedTextRun};

pub(super) struct LineRuns {
    pub(super) runs: Vec<Value>,
    pub(super) chars: Vec<Value>,
    pub(super) runs_with_boxes: usize,
}

pub(super) fn project_runs(item: &PositionedTextItem, line_start: u32) -> LineRuns {
    let (source_runs, run_char_ranges) = partition_text_runs(item);
    let mut chars = Vec::new();
    let mut runs_with_boxes = 0;
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
    LineRuns {
        runs,
        chars,
        runs_with_boxes,
    }
}

pub(super) fn partition_text_runs(
    item: &PositionedTextItem,
) -> (Vec<PositionedTextRun>, Vec<(usize, usize)>) {
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
