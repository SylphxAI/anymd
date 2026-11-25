//! Auto-mode selection, text extraction admission, and requested output flags.

use super::super::types::ReadPdfInput;

fn has_any_include(input: &ReadPdfInput) -> bool {
    input.include_metadata
        || input.include_page_count
        || input.include_full_text
        || input.include_markdown
        || input.include_chunks
        || input.include_elements
        || input.include_text_layer
        || input.include_document_map
        || input.include_images
        || input.include_tables
        || input.include_html
        || input.include_semantic_hints
        || input.include_outline
        || input.include_annotations
        || input.include_page_labels
        || input.include_page_geometry
        || input.include_permissions
        || input.include_form_fields
        || input.include_attachments
        || input.include_structure_tree
        || input.include_safety_findings
        || input.include_layout_diagnostics
        || input.include_document_ast
        || input.include_ocr_text_layer
        || input.include_visual_enrichments
        || input.include_trust_report
        || input.include_accessibility_report
}

pub(in crate::read_pdf) fn auto_enabled(input: &ReadPdfInput) -> bool {
    if input.auto_policy_resolved {
        false
    } else {
        input.auto.unwrap_or_else(|| !has_any_include(input))
    }
}

pub(in crate::read_pdf) fn requires_text_extraction(input: &ReadPdfInput) -> bool {
    auto_enabled(input)
        || input.include_full_text
        || input.include_markdown
        || input.include_chunks
        || input.include_elements
        || input.include_text_layer
        || input.include_document_map
        || input.include_tables
        || input.include_html
        || input.include_semantic_hints
        || input.include_safety_findings
        || input.include_layout_diagnostics
        || input.include_document_ast
        || input.include_ocr_text_layer
        || input.include_visual_enrichments
        || input.include_trust_report
        || input.include_accessibility_report
}

/// Requested output fields, distinct from intermediate data needed to build them.
pub(super) struct ExtractionOptions {
    pub(super) metadata: bool,
    pub(super) page_count: bool,
    pub(super) full_text: bool,
    pub(super) markdown: bool,
    pub(super) chunks: bool,
    pub(super) elements: bool,
    pub(super) semantic_hints: bool,
    pub(super) text_layer: bool,
    pub(super) document_map: bool,
    pub(super) tables: bool,
    pub(super) html: bool,
    pub(super) safety: bool,
    pub(super) layout: bool,
    pub(super) document_ast: bool,
    pub(super) trust: bool,
    pub(super) accessibility: bool,
    pub(super) outline: bool,
    pub(super) annotations: bool,
    pub(super) labels: bool,
    pub(super) geometry: bool,
    pub(super) permissions: bool,
    pub(super) form_fields: bool,
    pub(super) attachments: bool,
    pub(super) structure: bool,
    pub(super) images: bool,
    pub(super) ocr: bool,
    pub(super) visual: bool,
}

impl ExtractionOptions {
    pub(super) fn resolve(input: &ReadPdfInput) -> Self {
        // The JSON path resolves preset flags before reaching this function.
        // On the typed path, omitted auto enables auto mode only when no include flag is true.
        let auto = auto_enabled(input);
        let detail = input.auto_detail.as_deref().unwrap_or("balanced");
        let auto_full = auto && detail == "full";
        let auto_balanced = auto && matches!(detail, "balanced" | "full");
        let auto_fast = auto;
        Self {
            metadata: input.include_metadata || auto_fast,
            page_count: input.include_page_count || auto_fast,
            full_text: input.include_full_text || auto_full,
            markdown: input.include_markdown || auto_fast,
            chunks: input.include_chunks || auto_fast,
            elements: input.include_elements || auto_full,
            semantic_hints: input.include_semantic_hints || auto_fast,
            text_layer: input.include_text_layer || auto_full,
            document_map: input.include_document_map || auto_fast,
            tables: input.include_tables || auto_fast,
            html: input.include_html || auto_full,
            safety: input.include_safety_findings || auto_balanced || auto_full,
            layout: input.include_layout_diagnostics || auto_fast,
            document_ast: input.include_document_ast || auto_full,
            trust: input.include_trust_report || auto_balanced || auto_full,
            accessibility: input.include_accessibility_report || auto_balanced || auto_full,
            outline: input.include_outline || auto_full,
            annotations: input.include_annotations || auto_full,
            labels: input.include_page_labels || auto_full,
            geometry: input.include_page_geometry || auto_fast,
            permissions: input.include_permissions || auto_full,
            form_fields: input.include_form_fields || auto_full,
            attachments: input.include_attachments || auto_full,
            structure: input.include_structure_tree || auto_full,
            images: input.include_images,
            ocr: input.include_ocr_text_layer,
            visual: input.include_visual_enrichments,
        }
    }
}
