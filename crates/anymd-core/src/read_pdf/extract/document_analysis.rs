//! Build reports, visual candidates, and the document AST and map from page content.

use serde_json::{json, Value};

use crate::accessibility::{build_accessibility_report, AccessibilityInput};
use crate::document_twin::{
    build_document_ast, build_document_map, build_layout_diagnostics, build_safety_findings,
    build_trust_report, PageText,
};
use crate::visual_candidates::select_visual_enrichment_candidates;

use super::super::types::ReadPdfInput;
use super::{options::ExtractionOptions, page_content::PageContent, BuildSignals};

pub(super) struct DocumentAnalysis {
    pub(super) safety: Option<Value>,
    pub(super) layout: Option<Value>,
    pub(super) trust: Option<Value>,
    pub(super) accessibility: Option<Value>,
    pub(super) visual_candidates: Value,
    pub(super) document_ast: Option<Value>,
    pub(super) document_map: Option<Value>,
}

pub(super) fn analyze_document(
    pages: &[PageText],
    total_pages: u32,
    input: &ReadPdfInput,
    options: &ExtractionOptions,
    signals: &BuildSignals,
    content: &PageContent,
    warnings: &mut Vec<String>,
) -> DocumentAnalysis {
    let empty_array = json!([]);
    let safety = if options.safety || options.trust || options.document_map {
        Some(build_safety_findings(pages))
    } else {
        None
    };
    let layout = if options.layout || options.trust || options.document_map {
        Some(build_layout_diagnostics(pages))
    } else {
        None
    };
    let redaction = input
        .trust_report_redaction
        .as_deref()
        .unwrap_or("standard");
    let trust = if options.trust {
        Some(build_trust_report(
            pages,
            safety.as_ref().unwrap_or(&json!([])),
            layout.as_ref().unwrap_or(&json!([])),
            content.semantic_elements.as_ref().unwrap_or(&empty_array),
            signals.annotations.as_ref(),
            redaction,
        ))
    } else {
        None
    };
    let a11y = if options.accessibility {
        Some(build_accessibility_report(AccessibilityInput {
            pages,
            elements: content.semantic_elements.as_ref().unwrap_or(&empty_array),
            structure_trees: signals.accessibility_structure_trees.as_ref(),
            annotations: signals.annotations.as_ref(),
            form_fields: signals.form_fields.as_ref(),
            permissions: signals.permissions.as_ref(),
            mark_info: signals.mark_info.as_ref(),
            outline: signals.outline.as_ref(),
            structure_valid: signals.accessibility_structure_valid,
        }))
    } else {
        None
    };

    let visual_candidates = if options.visual {
        let geometry = signals.page_geometry.as_ref().and_then(Value::as_array);
        let outcome = select_visual_enrichment_candidates(
            content
                .semantic_elements
                .as_ref()
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            geometry.map(Vec::as_slice),
            input.max_visual_enrichments.unwrap_or(8) as usize,
        );
        warnings.extend(
            outcome
                .warnings
                .iter()
                .map(|warning| warning.message.clone()),
        );
        json!(outcome.candidates)
    } else {
        json!([])
    };

    let document_ast = if options.document_ast {
        Some(build_document_ast(
            pages,
            content.semantic_elements.as_ref().unwrap_or(&empty_array),
            content.internal_chunks.as_ref().unwrap_or(&empty_array),
            warnings,
            &empty_array,
        ))
    } else {
        None
    };

    let document_map = if options.document_map {
        Some(build_document_map(
            pages,
            total_pages,
            content.semantic_elements.as_ref().unwrap_or(&empty_array),
            content.internal_chunks.as_ref().unwrap_or(&empty_array),
            safety.as_ref().unwrap_or(&json!([])),
            layout.as_ref().unwrap_or(&json!([])),
            content.text_layer.as_ref().unwrap_or(&empty_array),
            signals.page_geometry.as_ref(),
            warnings,
            trust.as_ref(),
            a11y.as_ref(),
            &visual_candidates,
        ))
    } else {
        None
    };

    DocumentAnalysis {
        safety,
        layout,
        trust,
        accessibility: a11y,
        visual_candidates,
        document_ast,
        document_map,
    }
}
