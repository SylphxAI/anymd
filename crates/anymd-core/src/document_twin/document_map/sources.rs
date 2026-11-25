//! Normalized layer inputs and their advertised order.

use std::collections::BTreeMap;

use serde_json::Value;

use super::text::TextLayer;

pub(super) struct MapSources<'a> {
    pub(super) elements: &'a [Value],
    pub(super) chunks: &'a [Value],
    pub(super) safety: &'a [Value],
    pub(super) layout: &'a [Value],
    pub(super) geometry: &'a [Value],
    pub(super) text: TextLayer<'a>,
    pub(super) visual: &'a [Value],
}

pub(super) fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

pub(super) fn page_number(value: &Value) -> Option<u64> {
    value.get("page").and_then(Value::as_u64)
}

pub(super) fn page_indexes<'a>(
    values: impl IntoIterator<Item = &'a Value>,
) -> BTreeMap<u32, Vec<usize>> {
    let mut indexes = BTreeMap::<u32, Vec<usize>>::new();
    for (index, value) in values.into_iter().enumerate() {
        if let Some(page) = page_number(value).and_then(|page| u32::try_from(page).ok()) {
            indexes.entry(page).or_default().push(index);
        }
    }
    indexes
}

pub(super) fn element_kind(element: &Value) -> Option<&str> {
    element.get("type").and_then(Value::as_str)
}

impl MapSources<'_> {
    pub(super) fn layers(&self, has_trust: bool, has_accessibility: bool) -> Vec<&'static str> {
        let has_element = |kind| {
            self.elements
                .iter()
                .any(|element| element_kind(element) == Some(kind))
        };
        let has_semantic_hints = self.elements.iter().any(|element| {
            element_kind(element) == Some("text") && element.get("semantic_hint").is_some()
        });
        [
            (has_element("text"), "selectable_text"),
            (self.text.has_pages(), "text_layer"),
            (has_element("image"), "image_metadata"),
            (has_element("table"), "table_structure"),
            (!self.visual.is_empty(), "visual_region_candidates"),
            (has_semantic_hints, "semantic_hints"),
            (!self.chunks.is_empty(), "citation_chunks"),
            (!self.layout.is_empty(), "layout_diagnostics"),
            (!self.safety.is_empty(), "content_safety"),
            (has_trust, "trust_report"),
            (has_accessibility, "accessibility_report"),
            (!self.geometry.is_empty(), "page_geometry"),
        ]
        .into_iter()
        .filter_map(|(present, layer)| present.then_some(layer))
        .collect()
    }
}
