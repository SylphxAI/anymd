use serde_json::{json, Value};

use crate::fusion_util::utf16_len;

use super::OcrPage;

pub(super) fn fuse_document_map(map: &mut Value, _candidate_pages: &[u32], pages: &[OcrPage]) {
    let Some(object) = map.as_object_mut() else {
        return;
    };
    if !pages.is_empty() {
        let layers = object
            .entry("layers")
            .or_insert_with(|| json!([]))
            .as_array_mut();
        if let Some(layers) = layers {
            let layer = json!("ocr_text_layer");
            if !layers.contains(&layer) {
                layers.push(layer);
            }
        }
    }

    if let Some(map_pages) = object.get_mut("pages").and_then(Value::as_array_mut) {
        for map_page in map_pages {
            let Some(page_number) = map_page.get("page").and_then(Value::as_u64) else {
                continue;
            };
            let Some(ocr_page) = pages
                .iter()
                .find(|page| u64::from(page.page) == page_number)
            else {
                continue;
            };
            let Some(page_object) = map_page.as_object_mut() else {
                continue;
            };
            page_object.insert("ocr_text_chars".into(), json!(utf16_len(&ocr_page.text)));
            page_object.insert(
                "ocr_word_count".into(),
                json!(ocr_page.words.as_ref().map_or(0, Vec::len)),
            );
            page_object.insert(
                "ocr_source_render_evidence_id".into(),
                json!(ocr_page.source_render_evidence_id),
            );
            if let Some(confidence) = ocr_page.confidence {
                page_object.insert("ocr_confidence".into(), json!(confidence));
            }
        }
    }

    // needs_ocr_pages is layout-derived at map construction time (image/sparse
    // zero-text pages). Provider fusion must not overwrite it with OCR
    // candidate pages; only applied OCR pages and summary counters update here.
    let routing = object.entry("routing").or_insert_with(|| json!({}));
    if let Some(routing) = routing.as_object_mut() {
        routing.insert(
            "ocr_applied_pages".into(),
            json!(pages.iter().map(|page| page.page).collect::<Vec<_>>()),
        );
    }
    let summary = object.entry("summary").or_insert_with(|| json!({}));
    if let Some(summary) = summary.as_object_mut() {
        summary.insert("ocr_page_count".into(), json!(pages.len()));
        summary.insert(
            "ocr_text_chars".into(),
            json!(pages
                .iter()
                .map(|page| utf16_len(&page.text))
                .sum::<usize>()),
        );
    }
}
