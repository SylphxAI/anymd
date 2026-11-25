//! Whole-document summary, routing, and response envelope.

use serde_json::{json, Value};

use super::super::PageText;
use super::{
    accessibility::Accessibility,
    page,
    sources::{element_kind, page_indexes, page_number, MapSources},
    trust::Trust,
};

pub(super) fn assemble(
    sources: &MapSources<'_>,
    pages: &[PageText],
    total_pages: u32,
    warnings: &[String],
    trust: Option<&Value>,
    a11y: Option<&Value>,
) -> Value {
    let layers = sources.layers(trust.is_some(), a11y.is_some());
    let trust = Trust::new(trust);
    let accessibility = Accessibility::new(a11y);
    let selected_pages = page::selected(pages);
    let visual_indexes = page_indexes(sources.visual);
    let mapped_pages = page::project_pages(
        pages,
        &selected_pages,
        sources,
        &visual_indexes,
        &trust,
        &accessibility,
    );
    let (mut routing, average_layout_confidence, lowest_layout_confidence) =
        layout_routing(sources.layout);
    routing["visual_candidate_pages"] = json!(visual_indexes.keys().collect::<Vec<_>>());
    accessibility.append_routing(&mut routing);
    trust.append_routing(&mut routing);
    let mut summary = base_summary(
        sources,
        total_pages,
        &selected_pages,
        pages.len(),
        average_layout_confidence,
        lowest_layout_confidence,
    );
    if let Some(fields) = summary.as_object_mut() {
        accessibility.append_summary(fields);
        trust.append_summary(fields);
    }
    let mut output = json!({
        "version": "2026-06-15",
        "profile": "agent_document_map",
        "layers": layers,
        "pages": mapped_pages,
        "elements": sources.elements,
        "chunks": sources.chunks,
        "visual_enrichment_candidates": sources.visual,
        "visual_enrichments": [],
        "layout_diagnostics": sources.layout,
        "safety_findings": sources.safety,
        "routing": routing,
        "summary": summary,
    });
    if !warnings.is_empty() {
        output["warnings"] = json!(warnings);
    }
    output
}

fn layout_routing(layout_values: &[Value]) -> (Value, Option<f64>, Option<f64>) {
    let layout_confidences = layout_values
        .iter()
        .filter_map(|entry| entry.get("confidence").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let average_layout_confidence = (!layout_confidences.is_empty()).then(|| {
        let value = layout_confidences.iter().sum::<f64>() / layout_confidences.len() as f64;
        (value * 100.0).round() / 100.0
    });
    let lowest_layout_confidence = layout_confidences
        .iter()
        .copied()
        .reduce(f64::min)
        .map(|value| (value * 100.0).round() / 100.0);
    let low_confidence_pages = layout_values
        .iter()
        .filter(|entry| {
            entry
                .get("confidence")
                .and_then(Value::as_f64)
                .is_some_and(|value| value < 0.7)
        })
        .filter_map(page_number)
        .collect::<Vec<_>>();
    let image_or_sparse_pages = layout_values
        .iter()
        .filter(|entry| entry.get("profile").and_then(Value::as_str) == Some("image_or_sparse"))
        .filter_map(page_number)
        .collect::<Vec<_>>();
    let needs_ocr_pages = layout_values
        .iter()
        .filter(|entry| {
            (entry.get("profile").and_then(Value::as_str) == Some("image_or_sparse")
                || entry.get("item_count").and_then(Value::as_u64) == Some(0))
                && entry.get("text_item_count").and_then(Value::as_u64) == Some(0)
        })
        .filter_map(page_number)
        .collect::<Vec<_>>();
    let routing = json!({
        "low_confidence_pages": low_confidence_pages,
        "image_or_sparse_pages": image_or_sparse_pages,
        "needs_ocr_pages": needs_ocr_pages,
        "ocr_applied_pages": [],
    });
    (routing, average_layout_confidence, lowest_layout_confidence)
}

fn base_summary(
    sources: &MapSources<'_>,
    total_pages: u32,
    selected_pages: &[u32],
    processed_page_count: usize,
    average_layout_confidence: Option<f64>,
    lowest_layout_confidence: Option<f64>,
) -> Value {
    let text_element_count = sources
        .elements
        .iter()
        .filter(|element| element_kind(element) == Some("text"))
        .count();
    let image_element_count = sources
        .elements
        .iter()
        .filter(|element| element_kind(element) == Some("image"))
        .count();
    let table_element_count = sources
        .elements
        .iter()
        .filter(|element| element_kind(element) == Some("table"))
        .count();
    let mut visual_candidate_kind_counts = serde_json::Map::new();
    for candidate in sources.visual {
        let Some(kind) = candidate.get("target_element_type").and_then(Value::as_str) else {
            continue;
        };
        let count = visual_candidate_kind_counts
            .get(kind)
            .and_then(Value::as_u64)
            .unwrap_or(0);
        visual_candidate_kind_counts.insert(kind.to_owned(), json!(count + 1));
    }
    json!({
        "total_pages": total_pages,
        "selected_pages": selected_pages,
        "processed_page_count": processed_page_count,
        "element_count": sources.elements.len(),
        "text_element_count": text_element_count,
        "text_layer_page_count": sources.text.summary_metric("page_count"),
        "text_layer_run_count": sources.text.summary_metric("run_count"),
        "text_layer_line_count": sources.text.summary_metric("line_count"),
        "text_layer_word_count": sources.text.summary_metric("word_count"),
        "text_layer_char_count": sources.text.summary_metric("char_count"),
        "text_layer_runs_with_bounding_boxes": sources.text.summary_metric("runs_with_bounding_boxes"),
        "text_layer_lines_with_bounding_boxes": sources.text.summary_metric("lines_with_bounding_boxes"),
        "text_layer_words_with_bounding_boxes": sources.text.summary_metric("words_with_bounding_boxes"),
        "text_layer_chars_with_bounding_boxes": sources.text.summary_metric("chars_with_bounding_boxes"),
        "text_layer_runs_with_font_metadata": sources.text.summary_metric("runs_with_font_metadata"),
        "text_layer_runs_with_direction_metadata": sources.text.summary_metric("runs_with_direction_metadata"),
        "text_layer_runs_with_transform_metadata": sources.text.summary_metric("runs_with_transform_metadata"),
        "text_layer_runs_with_eol_metadata": sources.text.summary_metric("runs_with_eol_metadata"),
        "ocr_page_count": 0,
        "ocr_text_chars": 0,
        "image_element_count": image_element_count,
        "table_element_count": table_element_count,
        "visual_enrichment_candidate_count": sources.visual.len(),
        "visual_enrichment_candidate_kind_counts": visual_candidate_kind_counts,
        "visual_enrichment_count": 0,
        "visual_enrichment_kind_counts": {},
        "chunk_count": sources.chunks.len(),
        "safety_finding_count": sources.safety.len(),
        "average_layout_confidence": average_layout_confidence,
        "lowest_layout_confidence": lowest_layout_confidence,
    })
}
