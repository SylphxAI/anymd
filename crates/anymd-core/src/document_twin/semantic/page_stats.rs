//! Validated page bounds and per-page text-height statistics.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{PageSemanticBounds, PageSemanticStats};
use crate::document_twin::PageText;

pub(in crate::document_twin) fn page_semantic_bounds(
    page_geometry: Option<&Value>,
) -> BTreeMap<u32, PageSemanticBounds> {
    let mut bounds = BTreeMap::new();
    for geometry in page_geometry
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(page) = geometry.get("page").and_then(Value::as_u64) else {
            continue;
        };
        let Some(view_box) = geometry.get("view_box") else {
            continue;
        };
        let Some(left) = view_box.get("left").and_then(Value::as_f64) else {
            continue;
        };
        let Some(right) = view_box.get("right").and_then(Value::as_f64) else {
            continue;
        };
        let Some(bottom) = view_box.get("bottom").and_then(Value::as_f64) else {
            continue;
        };
        let Some(top) = view_box.get("top").and_then(Value::as_f64) else {
            continue;
        };
        if page > u64::from(u32::MAX)
            || ![left, right, bottom, top]
                .iter()
                .all(|value| value.is_finite())
            || right <= left
            || top <= bottom
        {
            continue;
        }
        bounds.insert(
            page as u32,
            PageSemanticBounds {
                left,
                right,
                bottom,
                top,
            },
        );
    }
    bounds
}

pub(in crate::document_twin) fn page_semantic_stats(
    page: &PageText,
    bounds: Option<PageSemanticBounds>,
) -> PageSemanticStats {
    let mut heights = page
        .positioned_items
        .iter()
        .filter(|item| !item.text.trim().is_empty())
        .filter_map(|item| item.bounding_box.map(|box_| box_.top - box_.bottom))
        .filter(|height| height.is_finite() && *height != 0.0)
        .collect::<Vec<_>>();
    heights.sort_by(f64::total_cmp);
    let midpoint = heights.len() / 2;
    let median_height = if heights.is_empty() {
        0.0
    } else if heights.len().is_multiple_of(2) {
        (heights[midpoint - 1] + heights[midpoint]) / 2.0
    } else {
        heights[midpoint]
    };
    PageSemanticStats {
        max_height: heights.last().copied().unwrap_or(0.0),
        median_height,
        text_item_count: heights.len(),
        bounds,
    }
}
