//! Assemble line and page projections, totals, and warnings for a document.

use serde_json::{json, Value};

use super::super::PageText;
use super::{
    runs::{project_runs, LineRuns},
    utf16_len,
    words::item_words,
};
use crate::text_index::PositionedTextItem;

/// Build the bounded selectable-text geometry projection. Coordinates are
/// source-derived item boxes with uniformly estimated UTF-16 character boxes,
/// not glyph outlines.
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
            let LineRuns {
                runs,
                chars,
                runs_with_boxes: line_runs_with_boxes,
            } = project_runs(item, line_start);
            run_count += runs.len();
            runs_with_boxes += line_runs_with_boxes;
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
