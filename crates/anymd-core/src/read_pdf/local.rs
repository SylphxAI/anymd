//! Cached PDF loading, text extraction, and page selection.

use std::path::Path;

use serde_json::Value;

use crate::cos_document::ParsedPdf;
use crate::read_result_cache::ReadResultCache;
use crate::text_index::extract_pdf_text_from_document;

use super::extract::{auto_enabled, build_data, requires_text_extraction};
use super::pages::{evenly_sample_pages, select_pages};
use super::signals::collect_signals;
use super::types::{ReadPdfError, ReadPdfInput, ReadPdfSourceResult};

pub(super) const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn read_local_pdf_filtered(
    path: &Path,
    input: &ReadPdfInput,
    pages_spec: &Option<Value>,
    source_label: &str,
    cache: &ReadResultCache,
) -> Result<ReadPdfSourceResult, ReadPdfError> {
    if let Some(mut cached) = cache.get(path, input, pages_spec) {
        // Preserve the caller's source label (path or original display form).
        cached.source = source_label.to_string();
        return Ok(cached);
    }
    let parsed = ParsedPdf::load(path, DEFAULT_MAX_FILE_BYTES)?;
    let requires_text = requires_text_extraction(input);
    let (pages, mut pdf_info) = if requires_text {
        let extracted = extract_pdf_text_from_document(&parsed.document)?;
        (extracted.pages, extracted.info)
    } else {
        (
            (0..parsed.pages.len().max(1))
                .map(|_| crate::text_index::ExtractedPageText {
                    text: String::new(),
                    items: Vec::new(),
                    positioned_items: Vec::new(),
                })
                .collect(),
            crate::text_index::read_pdf_info(&parsed.document),
        )
    };
    // Encrypt dictionary is removed after empty-password decrypt; restore
    // pdf.js EncryptFilterName from pre-decrypt encryption facts when needed.
    if pdf_info.encrypt_filter_name.is_none() {
        if let Some(filter_name) = parsed
            .encryption_facts
            .as_ref()
            .and_then(|facts| facts.filter_name.clone())
        {
            pdf_info.encrypt_filter_name = Some(filter_name);
        }
    }
    pdf_info.is_linearized = parsed.is_linearized;
    let total_pages = parsed.pages.len().max(1) as u32;
    let explicit_pages = crate::pages::parse_page_spec::<ReadPdfError>(pages_spec, true)?;
    let auto_pages = if explicit_pages.is_none()
        && auto_enabled(input)
        && input.auto_detail.as_deref().unwrap_or("balanced") != "full"
    {
        Some(evenly_sample_pages(
            total_pages,
            input.sample_pages.unwrap_or(5),
        ))
    } else {
        None
    };
    let requested_pages = explicit_pages.as_deref().or(auto_pages.as_deref());
    let (selected, invalid_pages) = select_pages(&pages, requested_pages);
    let selected_page_numbers = selected.iter().map(|page| page.page).collect::<Vec<_>>();
    let signals = collect_signals(
        parsed,
        input,
        total_pages,
        &selected_page_numbers,
        &invalid_pages,
    );
    let data = build_data(
        &selected,
        total_pages,
        input,
        Some(&pdf_info),
        explicit_pages.is_some(),
        signals,
    );
    let result = ReadPdfSourceResult {
        source: source_label.to_string(),
        success: true,
        error: None,
        data: Some(data),
    };
    cache.store(path, input, pages_spec, &result);
    Ok(result)
}
