//! Optional page and document signals for a selected set of PDF pages.

use serde_json::json;

use crate::cos_document::ParsedPdf;

use super::extract::{auto_enabled, BuildSignals};
use super::types::ReadPdfInput;

pub(super) fn collect_signals(
    parsed: ParsedPdf,
    input: &ReadPdfInput,
    total_pages: u32,
    selected_page_numbers: &[u32],
    invalid_pages: &[u32],
) -> BuildSignals {
    let auto = auto_enabled(input);
    let want_geometry = input.include_page_geometry
        || auto
        || input.include_semantic_hints
        || input.include_document_map
        || input.include_document_ast
        || input.include_visual_enrichments
        || input.include_trust_report;
    let want_private_a11y = input.include_accessibility_report
        || (auto
            && matches!(
                input.auto_detail.as_deref().unwrap_or("balanced"),
                "balanced" | "full"
            ));
    let want_annotations = input.include_annotations
        || input.include_trust_report
        || (auto
            && matches!(
                input.auto_detail.as_deref().unwrap_or("balanced"),
                "balanced" | "full"
            ))
        || want_private_a11y;
    let signals = crate::page_signals::extract_page_signals(
        &parsed.document,
        &parsed.pages,
        selected_page_numbers,
        want_geometry,
        want_annotations,
    );
    let geometry = (!signals.geometry.is_empty()).then(|| json!(signals.geometry));
    let annotations = (!signals.annotations.is_empty()).then(|| json!(signals.annotations));
    let auto_full = auto && input.auto_detail.as_deref() == Some("full");
    let catalog_signals = crate::catalog_signals::extract_catalog_signals(
        &parsed.document,
        parsed.encryption_facts,
        total_pages,
        crate::catalog_signals::CatalogSignalRequest {
            page_labels: input.include_page_labels || auto_full,
            permissions: input.include_permissions || auto_full || want_private_a11y,
            outline: input.include_outline || auto_full || want_private_a11y,
        },
    );
    let form_attachment_signals = crate::form_attachment_signals::extract_form_attachment_signals(
        &parsed.document,
        &parsed.pages,
        input.include_form_fields || auto_full || want_private_a11y,
        input.include_attachments || auto_full,
    );
    let want_private_structure = input.include_structure_tree
        || input.include_accessibility_report
        || (auto
            && matches!(
                input.auto_detail.as_deref().unwrap_or("balanced"),
                "balanced" | "full"
            ));
    let checked_structure = want_private_structure.then(|| {
        crate::structure_signals::extract_structure_trees_checked(
            &parsed.document,
            &parsed.pages,
            selected_page_numbers,
        )
    });
    let accessibility_structure_valid = match checked_structure.as_ref() {
        None | Some(Ok(None)) => true,
        Some(Ok(Some(value))) => value.complete,
        Some(Err(())) => false,
    };
    let structure_extraction = checked_structure.and_then(Result::ok).flatten();
    let structure_trees = structure_extraction
        .as_ref()
        .and_then(|value| (!value.trees.is_empty()).then(|| json!(value.trees)));
    let accessibility_structure_trees = structure_extraction
        .as_ref()
        .filter(|value| value.complete)
        .and_then(|value| (!value.trees.is_empty()).then(|| json!(value.trees)));
    let image_signals = if input.include_images {
        crate::image_signals::extract_image_signals(
            &parsed.document,
            &parsed.pages,
            selected_page_numbers,
        )
    } else {
        crate::image_signals::ImageSignals::default()
    };
    let mut signal_warnings = signals.warnings;
    signal_warnings.extend(form_attachment_signals.warnings);
    signal_warnings.extend(image_signals.warnings);
    if !invalid_pages.is_empty() {
        signal_warnings.push(format!(
            "Requested page numbers {} exceed total pages ({total_pages}).",
            invalid_pages
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    BuildSignals {
        page_geometry: geometry,
        annotations,
        page_labels: catalog_signals.page_labels.map(|value| json!(value)),
        permissions: catalog_signals.permissions.map(|value| json!(value)),
        mark_info: catalog_signals.mark_info.map(|value| json!(value)),
        outline: catalog_signals.outline.map(|value| json!(value)),
        form_fields: form_attachment_signals
            .form_fields
            .map(|value| json!(value)),
        attachments: form_attachment_signals
            .attachments
            .map(|value| json!(value)),
        structure_trees,
        accessibility_structure_trees,
        accessibility_structure_valid,
        images: input.include_images.then(|| json!(image_signals.images)),
        has_catalog_metadata: parsed
            .document
            .catalog()
            .ok()
            .and_then(|catalog| catalog.get(b"Metadata").ok())
            .is_some(),
        warnings: signal_warnings,
    }
}
