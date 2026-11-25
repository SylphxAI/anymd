//! Extraction policy and construction of read_pdf output from selected pages and PDF signals.

use serde_json::Value;

use crate::text_index::PdfInfo;

use super::types::{ReadPdfData, ReadPdfInput};

mod document_analysis;
mod options;
mod page_content;
mod response;

use document_analysis::analyze_document;
use options::ExtractionOptions;
use page_content::build_page_content;
use response::{assemble_response, PageSelection};

pub(super) use options::{auto_enabled, requires_text_extraction};

#[derive(Default)]
pub(super) struct BuildSignals {
    pub(super) page_geometry: Option<Value>,
    pub(super) annotations: Option<Value>,
    pub(super) page_labels: Option<Value>,
    pub(super) permissions: Option<Value>,
    pub(super) mark_info: Option<Value>,
    pub(super) outline: Option<Value>,
    pub(super) form_fields: Option<Value>,
    pub(super) attachments: Option<Value>,
    pub(super) structure_trees: Option<Value>,
    pub(super) accessibility_structure_trees: Option<Value>,
    pub(super) accessibility_structure_valid: bool,
    pub(super) images: Option<Value>,
    /// Catalog /Metadata stream present. TS emits `metadata` only then.
    pub(super) has_catalog_metadata: bool,
    pub(super) warnings: Vec<String>,
}

pub(super) fn build_data(
    pages: &[crate::document_twin::PageText],
    total_pages: u32,
    input: &ReadPdfInput,
    pdf_info: Option<&PdfInfo>,
    explicit_page_selection: bool,
    signals: BuildSignals,
) -> ReadPdfData {
    let mut signals = signals;
    let options = ExtractionOptions::resolve(input);
    let mut warnings = std::mem::take(&mut signals.warnings);
    if options.ocr {
        warnings.push(crate::ocr_fusion::OCR_STUB_WARNING.into());
    }
    if options.visual {
        warnings
            .push("Visual enrichment skipped: analyze_regions provider is not_configured.".into());
    }

    let content = build_page_content(
        pages,
        &options,
        signals.page_geometry.as_ref(),
        signals.images.as_ref(),
        &mut warnings,
    );
    let analysis = analyze_document(
        pages,
        total_pages,
        input,
        &options,
        &signals,
        &content,
        &mut warnings,
    );
    signals.warnings = warnings;
    assemble_response(
        PageSelection {
            pages,
            total_pages,
            explicit: explicit_page_selection,
        },
        &options,
        pdf_info,
        signals,
        content,
        analysis,
    )
}
