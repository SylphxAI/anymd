//! Page preparation and traversal in reading order.

use std::collections::HashSet;

use crate::extract::{Glyph, RawPage};
use crate::margins::{in_margin, is_page_number, margin_key};
use crate::reading::reading_regions;
use crate::rows::{row_text, rows_of, segments_of_row, Segment};
use crate::tables::ruled::{ruled_tables, Ruled, RuledTable};

use super::{region_blocks, Block, PageTables};

pub(crate) fn layout_page(
    glyphs: &[Glyph],
    page: &RawPage,
    body: f64,
    repeated: &HashSet<String>,
) -> Vec<Block> {
    let (ruled, glyphs) = ruled_tables(&page.rules, glyphs.to_vec());
    let mut segments = Vec::new();
    for row in rows_of(glyphs) {
        segments.extend(segments_of_row(row));
    }
    if page.ocr {
        for segment in &mut segments {
            segment.mono = None;
        }
    }
    segments.retain(|segment| {
        !(in_margin(segment, page)
            && (repeated.contains(&margin_key(&segment.text)) || is_page_number(&segment.text)))
    });
    // Each ruled table takes part in reading order as one placeholder.
    for (index, table) in ruled.iter().enumerate() {
        segments.push(Segment {
            x0: table.x0,
            x1: table.x1,
            base: table.top - body,
            top: table.top,
            bottom: table.bottom,
            size: body,
            text: String::new(),
            mono: None,
            words: Vec::new(),
            table: Some(index),
        });
    }
    // Each embedded image takes part the same way, after the ruled tables.
    let mut found: Vec<Option<RuledTable>> = ruled.into_iter().map(Some).collect();
    for figure in &page.figures {
        let [x0, y0, x1, y1] = figure.bbox;
        segments.push(Segment {
            x0,
            x1,
            base: y1 - body,
            top: y1,
            bottom: y0,
            size: body,
            text: String::new(),
            mono: None,
            words: Vec::new(),
            table: Some(found.len()),
        });
        found.push(Some(RuledTable {
            x0,
            bottom: y0,
            x1,
            top: y1,
            content: Ruled::Image(figure.markdown.clone()),
        }));
    }
    let mut spaces: Vec<f64> = segments
        .iter()
        .flat_map(|s| {
            s.words
                .windows(2)
                .map(move |pair| (pair[1].x0 - pair[0].x1) / s.size.max(0.1))
        })
        .filter(|gap| *gap > 0.0 && *gap < 1.2)
        .collect();
    spaces.sort_by(f64::total_cmp);
    let mut tables = PageTables {
        found,
        rules: page.rules.clone(),
        word_space: spaces.get(spaces.len() / 2).copied().unwrap_or(0.25),
        ocr: page.ocr,
    };
    let mut blocks = Vec::new();
    for region in reading_regions(segments, body, 0) {
        region_blocks(region, body, &mut tables, &mut blocks);
    }
    if !page.rotated.is_empty() {
        let mut lines = Vec::new();
        for row in rows_of(page.rotated.clone()) {
            let text = row_text(&segments_of_row(row));
            if !text.is_empty() {
                lines.push(text);
            }
        }
        if !lines.is_empty() {
            blocks.push(Block::Paragraph {
                text: lines.join(" "),
                size: 0.0,
                lines: 1,
            });
        }
    }
    blocks
}
