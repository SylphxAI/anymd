//! Markdown from OCR words: the same layout as a text PDF (reading order,
//! paragraphs, headings, tables), fed with word boxes instead of glyphs.

use std::collections::{BTreeMap, HashSet};

use crate::blocks::{layout_page, Block};
use crate::extract::{Glyph, RawPage};
use crate::render::{heading_levels, is_size_heading, render_blocks};

/// One word placed on a page image: its box in pixels (y grows downward),
/// the text line it belongs to (any key shared by the words of one line),
/// and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedWord {
    pub x0: f64,
    pub top: f64,
    pub x1: f64,
    pub bottom: f64,
    pub line: u64,
    pub text: String,
}

/// Lay out the words OCR found on one page image. `height` is the image
/// height in pixels and `points_per_pixel` converts pixels to PDF points
/// (72 / dpi), so the layout's thresholds, which are in points, apply.
pub fn words_to_markdown(words: &[PlacedWord], height: f64, points_per_pixel: f64) -> String {
    let scale = if points_per_pixel.is_finite() && points_per_pixel > 0.0 {
        points_per_pixel
    } else {
        1.0
    };
    // A line's height stands in for its font size. Lines within a third of
    // the typical height are body text, so OCR's uneven boxes (a line of
    // capitals, a line with descenders) do not become headings.
    let mut lines: BTreeMap<u64, (f64, f64)> = BTreeMap::new();
    for word in words {
        let entry = lines.entry(word.line).or_insert((word.top, word.bottom));
        entry.0 = entry.0.min(word.top);
        entry.1 = entry.1.max(word.bottom);
    }
    let mut heights: Vec<f64> = lines
        .values()
        .map(|(t, b)| (b - t) * scale)
        .filter(|h| *h > 0.0)
        .collect();
    heights.sort_by(f64::total_cmp);
    let Some(&body) = heights.get(heights.len() / 2) else {
        return String::new();
    };
    let size_of = |line: u64| {
        let (top, bottom) = lines[&line];
        let size = (bottom - top) * scale;
        if size < body * 1.35 {
            body
        } else {
            size
        }
    };
    // Each word becomes evenly spaced glyphs, so the gaps between words are
    // the only gaps and word spaces are found as for a text PDF.
    let mut glyphs = Vec::new();
    for word in words {
        let chars: Vec<char> = word.text.chars().collect();
        if chars.is_empty() || !(word.x1 > word.x0) {
            continue;
        }
        let size = size_of(word.line);
        let (_, line_bottom) = lines[&word.line];
        let base = (height - line_bottom) * scale + size * 0.2;
        let step = (word.x1 - word.x0) * scale / chars.len() as f64;
        for (index, ch) in chars.iter().enumerate() {
            let x0 = word.x0 * scale + step * index as f64;
            glyphs.push(Glyph {
                x0,
                x1: x0 + step,
                base,
                size,
                text: ch.to_string(),
                space: false,
            });
        }
    }
    let page = RawPage {
        number: 1,
        bottom: 0.0,
        top: height * scale,
        glyphs: Ok(Vec::new()),
        rotated: Vec::new(),
        rules: Vec::new(),
        ocr: true,
    };
    let mut blocks = layout_page(&glyphs, &page, body, &HashSet::new());
    for block in &mut blocks {
        match block {
            Block::Paragraph { text, size, .. } => {
                end_with_period(text);
                // A taller OCR box on a long line is a smudge or a stamp,
                // not a heading.
                if text.chars().count() > 60 {
                    *size = body;
                }
            }
            Block::ListItem(text) => end_with_period(text),
            _ => {}
        }
    }
    let sizes: Vec<f64> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { size, text, .. } if is_size_heading(*size, body, text) => {
                Some(*size)
            }
            _ => None,
        })
        .collect();
    let mut first_heading = None;
    render_blocks(&blocks, body, &heading_levels(&sizes), &mut first_heading)
}

/// OCR often reads a typewritten full stop as a comma. A paragraph never
/// ends with a comma, so a final comma after a word is a full stop.
fn end_with_period(text: &mut String) {
    let trimmed = text.trim_end();
    if trimmed.len() < 20 || !trimmed.ends_with(',') {
        return;
    }
    let before = trimmed[..trimmed.len() - 1].chars().last();
    if before.is_some_and(|c| c.is_alphanumeric() || c == ')' || c == '%') {
        let cut = trimmed.len() - 1;
        text.truncate(cut);
        text.push('.');
    }
}
