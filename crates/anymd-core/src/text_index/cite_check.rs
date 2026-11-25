//! Deterministic quote/location support. This is not a semantic truth check.
use super::{PositionedTextItem, TextBoundingBox};
use serde::{Deserialize, Serialize};

mod geometry;
mod matching;

#[cfg(test)]
mod tests;

/// Coordinates are rounded to 1/10,000 PDF unit by the text owner. Allow only
/// that rounding error, never a search radius or configurable padding.
pub const CITE_ROUNDING_TOLERANCE: f64 = 0.0001;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CiteNormalization {
    #[default]
    None,
    WhitespaceV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiteSupport {
    pub observed_text: String,
    /// Original reading-order UTF-16 offsets, not normalized offsets.
    pub start: u32,
    pub end: u32,
    pub boxes: Vec<TextBoundingBox>,
    pub geometry_level: String,
    /// One granularity label per supporting box; mixed spans never lose labels.
    pub geometry_levels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiteDecision {
    pub verdict: String,
    pub locations: Vec<CiteSupport>,
}

pub fn valid_cite_box(b: TextBoundingBox) -> bool {
    [b.left, b.bottom, b.right, b.top]
        .into_iter()
        .all(f64::is_finite)
        && b.right > b.left
        && b.top > b.bottom
}

/// Items are contiguous in the existing text-index reading order. A coarse
/// item's whole box must fit; native estimated characters must cover every
/// contributing non-whitespace UTF-16 unit. Partial unions never prove support.
pub fn check_cite_items(
    items: &[PositionedTextItem],
    quote: &str,
    bounds: TextBoundingBox,
    normalization: CiteNormalization,
    complete: bool,
    coarse_level: &str,
) -> CiteDecision {
    check_cite_items_with_separator(
        items,
        quote,
        bounds,
        normalization,
        complete,
        coarse_level,
        "\n",
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn check_cite_items_with_separator(
    items: &[PositionedTextItem],
    quote: &str,
    bounds: TextBoundingBox,
    normalization: CiteNormalization,
    complete: bool,
    coarse_level: &str,
    separator: &str,
    coarse_levels: Option<&[&str]>,
) -> CiteDecision {
    let text = items
        .iter()
        .map(|i| i.text.as_str())
        .collect::<Vec<_>>()
        .join(separator);
    let geometry = geometry::Geometry::new(items, bounds, separator, coarse_level, coarse_levels);
    matching::decide(&text, quote, normalization, complete, &geometry)
}
