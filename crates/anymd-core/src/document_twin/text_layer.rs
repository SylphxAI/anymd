//! Selectable text layer: lines, words, and runs with UTF-16 offsets and geometry.

mod builder;
mod runs;
mod words;

#[cfg(test)]
mod tests;

use crate::text_index::TextBoundingBox;
pub use builder::build_text_layer;

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
