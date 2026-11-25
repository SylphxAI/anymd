use serde_json::json;

use crate::fusion_util::{append_unique_warning, remove_stub_warning, round_ratio, utf16_len};
use crate::read_pdf::{rebuild_structured_outputs, ReadPdfResponse};

use super::document_map::fuse_document_map;
use super::{SourceOcrOutcome, OCR_STUB_WARNING};

/// Fuse normalized provider outcomes into their matching read results.
///
/// Source identity is ordinal by design: duplicate source labels are valid.
pub fn fuse_ocr_outcomes(response: &mut ReadPdfResponse, outcomes: Vec<SourceOcrOutcome>) {
    for outcome in outcomes {
        let Some(result) = response.results.get_mut(outcome.source_index) else {
            continue;
        };
        let Some(data) = result.data.as_mut().filter(|_| result.success) else {
            continue;
        };
        remove_stub_warning(&mut data.warnings, OCR_STUB_WARNING);
        data.ocr_text_layer = None;

        if let Some(error) = outcome.error {
            if let Some(context) = data.structured_fusion_context.clone() {
                let selectable_tables = context.selectable_tables.clone();
                rebuild_structured_outputs(data, &context, &selectable_tables);
            }
            if let Some(map) = data.document_map.as_mut() {
                fuse_document_map(map, &data.ocr_candidate_pages, &[]);
            }
            append_unique_warning(
                &mut data.warnings,
                format!("OCR text layer unavailable: {error}"),
            );
            continue;
        }
        if outcome.pages.is_empty() {
            if let Some(context) = data.structured_fusion_context.clone() {
                let selectable_tables = context.selectable_tables.clone();
                rebuild_structured_outputs(data, &context, &selectable_tables);
            }
            if let Some(map) = data.document_map.as_mut() {
                fuse_document_map(map, &data.ocr_candidate_pages, &[]);
            }
            continue;
        }

        let page_warnings = outcome
            .pages
            .iter()
            .flat_map(|page| page.warnings.clone().unwrap_or_default())
            .collect::<Vec<_>>();
        let mut layer_warnings = outcome.warnings.clone();
        layer_warnings.extend(page_warnings);
        let confidences = outcome
            .pages
            .iter()
            .filter_map(|page| page.confidence)
            .collect::<Vec<_>>();
        let average_confidence = (!confidences.is_empty())
            .then(|| round_ratio(confidences.iter().sum::<f64>() / confidences.len() as f64));
        let source_render_count = outcome
            .pages
            .iter()
            .map(|page| page.source_render_evidence_id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len();
        let mut summary = json!({
            "page_count": outcome.pages.len(),
            "text_chars": outcome.pages.iter().map(|page| utf16_len(&page.text)).sum::<usize>(),
            "word_count": outcome.pages.iter().map(|page| page.words.as_ref().map_or(0, Vec::len)).sum::<usize>(),
            "words_with_bounding_boxes": outcome.pages.iter().flat_map(|page| page.words.as_deref().unwrap_or_default()).filter(|word| word.bounding_box.is_some()).count(),
            "source_render_count": source_render_count,
        });
        if let Some(confidence) = average_confidence {
            summary["average_confidence"] = json!(confidence);
        }
        let mut layer = json!({
            "profile": "ocr_text_layer",
            "pages": outcome.pages,
            "summary": summary,
        });
        if !layer_warnings.is_empty() {
            layer["warnings"] = json!(layer_warnings);
        }
        data.ocr_text_layer = Some(layer);
        for warning in outcome.warnings {
            append_unique_warning(&mut data.warnings, warning);
        }

        if let Some(context) = data.structured_fusion_context.clone() {
            let merged_tables =
                crate::ocr_tables::merge_ocr_tables(&outcome.pages, &context.selectable_tables);
            rebuild_structured_outputs(data, &context, &merged_tables);
        }
        if let Some(map) = data.document_map.as_mut() {
            fuse_document_map(map, &data.ocr_candidate_pages, &outcome.pages);
        }
    }
}
