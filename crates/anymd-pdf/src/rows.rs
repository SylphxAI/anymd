//! Rows of glyphs and the segments (runs of words) inside them.

mod grouping;
mod metrics;
mod segments;
mod spacing;

use crate::extract::Glyph;

pub(crate) use grouping::rows_of;
pub(crate) use metrics::{body_font_size, dominant_size, monospace};
pub(crate) use segments::segments_of_row;
pub use spacing::infer_word_spaces;
pub(crate) use spacing::{is_cjk, space_scale, space_thresholds, wants_word_space};

/// A horizontal run of text on one baseline with no large gap inside it.
#[derive(Debug, Clone)]
pub(crate) struct Segment {
    pub(crate) x0: f64,
    pub(crate) x1: f64,
    pub(crate) base: f64,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
    pub(crate) size: f64,
    pub(crate) text: String,
    /// Some(true) when glyph advances are uniform (a monospace font).
    pub(crate) mono: Option<bool>,
    /// The words of `text` (split at inferred spaces) with their extents.
    pub(crate) words: Vec<Word>,
    /// A stand-in for the ruled table with this index, placed in reading
    /// order like text.
    pub(crate) table: Option<usize>,
}

/// One word of a segment and its horizontal extent.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Word {
    pub(crate) x0: f64,
    pub(crate) x1: f64,
    pub(crate) text: String,
}

impl Segment {
    pub(crate) fn chars(&self) -> usize {
        self.text.chars().count()
    }
}

/// One glyph for [`infer_word_spaces`]: its extent along the text direction
/// (start and advance end), its font size, and its text.
#[derive(Debug, Clone, Copy)]
pub struct SpacingGlyph<'a> {
    pub x0: f64,
    pub x1: f64,
    pub size: f64,
    pub text: &'a str,
}

pub(crate) fn new_segment(glyph: &Glyph, dominant: f64) -> Segment {
    Segment {
        x0: glyph.x0,
        x1: glyph.x1,
        base: glyph.base,
        top: glyph.base + glyph.size * 0.8,
        bottom: glyph.base - glyph.size * 0.2,
        size: if dominant > 0.0 { dominant } else { glyph.size },
        text: glyph.text.clone(),
        mono: None,
        words: vec![Word {
            x0: glyph.x0,
            x1: glyph.x1,
            text: glyph.text.clone(),
        }],
        table: None,
    }
}

/// The text of a row: its segments joined by spaces.
pub(crate) fn row_text(row: &[Segment]) -> String {
    let mut text = String::new();
    for segment in row {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&segment.text);
    }
    text
}
