//! Semantic role hints (heading, caption, list, header/footer) for text elements.

mod classification;
mod page_stats;
mod running_text;

use serde::Serialize;

pub(super) use classification::{caption_prefix_pattern, semantic_hint};
pub(super) use page_stats::{page_semantic_bounds, page_semantic_stats};

#[derive(Debug, Clone, Serialize)]
pub(super) struct SemanticHint {
    role: &'static str,
    confidence: f64,
    signals: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PageSemanticBounds {
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PageSemanticStats {
    max_height: f64,
    median_height: f64,
    text_item_count: usize,
    bounds: Option<PageSemanticBounds>,
}

#[cfg(test)]
mod tests;
