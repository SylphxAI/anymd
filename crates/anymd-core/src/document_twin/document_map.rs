//! Document map: per-page routing indexes and summary for agents.

mod accessibility;
mod output;
mod page;
mod sources;
mod text;
mod trust;

#[cfg(test)]
mod tests;

use serde_json::Value;

use super::PageText;
use sources::{array, MapSources};
use text::TextLayer;

#[allow(clippy::too_many_arguments)]
pub fn build_document_map(
    pages: &[PageText],
    total_pages: u32,
    elements: &Value,
    chunks: &Value,
    safety: &Value,
    layout: &Value,
    text_layer: &Value,
    page_geometry: Option<&Value>,
    warnings: &[String],
    trust: Option<&Value>,
    a11y: Option<&Value>,
    visual_candidates: &Value,
) -> Value {
    let sources = MapSources {
        elements: array(elements),
        chunks: array(chunks),
        safety: array(safety),
        layout: array(layout),
        geometry: page_geometry.map_or(&[], array),
        text: TextLayer::new(text_layer),
        visual: array(visual_candidates),
    };
    output::assemble(&sources, pages, total_pages, warnings, trust, a11y)
}
