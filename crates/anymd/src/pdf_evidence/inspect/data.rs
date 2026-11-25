use anymd_core::text_index::{TextIndexError, extract_page_texts};
use anymd_core::{ENGINE_NAME, ENGINE_VERSION};
use serde_json::{Value, json};
use std::path::Path;

use crate::schema::PdfSource;

use super::sampling::sample_pages;
use super::{DEFAULT_MAX_FILE_BYTES, INSPECT_ROUTE};

pub(super) fn inspect_source(
    path: &Path,
    targets: Option<&[u32]>,
    args: &Value,
    source: &PdfSource,
) -> Result<Value, TextIndexError> {
    let pages = extract_page_texts(path, DEFAULT_MAX_FILE_BYTES)?;
    let num_pages = pages.len().max(1) as u32;
    let sample_count = args
        .get("sample_pages")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 20) as usize;
    let sampled_pages = sample_pages(num_pages, targets, sample_count);
    let (_, document_signals) =
        anymd_core::inspect_document_signal_presence(path, DEFAULT_MAX_FILE_BYTES, &sampled_pages)?;
    let page_signals = sampled_pages
        .iter()
        .filter_map(|page_number| {
            pages.get(*page_number as usize - 1).map(|page| {
                let text_chars = page.trim().chars().count();
                let text_items = page
                    .lines()
                    .filter(|value| !value.trim().is_empty())
                    .count();
                json!({
                    "page": page_number,
                    "text_chars": text_chars,
                    "text_items": text_items,
                    "estimated_tokens": text_chars.div_ceil(4),
                    "image_paint_operations": 0,
                    "likely_scanned": false,
                    "low_text_density": text_chars < 80,
                })
            })
        })
        .collect::<Vec<_>>();
    let profile = if page_signals
        .iter()
        .any(|signal| signal["text_chars"].as_u64().unwrap_or(0) >= 80)
    {
        "digital_text"
    } else {
        "low_text_or_form"
    };
    let has_structure_tree = document_signals["has_structure_tree"]
        .as_bool()
        .unwrap_or(false);
    let mut read_pdf_arguments = json!({
        "sources": [source],
        "include_metadata": true,
        "include_page_count": true,
        "include_page_geometry": true,
    });
    if has_structure_tree {
        read_pdf_arguments["include_structure_tree"] = json!(true);
    }

    Ok(json!({
        "profile": profile,
        "num_pages": num_pages,
        "sampled_pages": sampled_pages,
        "page_signals": page_signals,
        "document_signals": document_signals,
        "recommendation": {
            "workflow": if profile == "digital_text" { "agentic_rag" } else { "metadata_review" },
            "needs_ocr": false,
            "reason": if profile == "digital_text" {
                "Sampled pages expose selectable text; the agent document map, citation chunks, semantic hints, table extraction, safety findings, and visual enrichment fusion are the highest-value next read_pdf options when providers are ready."
            } else {
                "Sampled pages expose limited text; inspect metadata, forms, attachments, structure, and selected pages before running a heavier extraction."
            },
            "read_pdf_arguments": read_pdf_arguments,
        },
        "route": INSPECT_ROUTE,
        "engine": {
            "name": ENGINE_NAME,
            "version": ENGINE_VERSION,
        },
    }))
}
