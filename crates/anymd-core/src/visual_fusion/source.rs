use serde_json::{json, Value};

use super::{document_map::fuse_document_map, SourceVisualOutcome, VISUAL_STUB_WARNING};
use crate::fusion_util::{append_unique_warning, remove_stub_warning};
use crate::read_pdf::ReadPdfData;

pub(super) fn fuse_source(
    data: &mut ReadPdfData,
    source_label: &str,
    outcome: SourceVisualOutcome,
) {
    if outcome.provider_ready {
        remove_stub_warning(&mut data.warnings, VISUAL_STUB_WARNING);
    }

    data.visual_enrichments = None;

    if let Some(error) = outcome.error {
        for warning in outcome.warnings {
            append_unique_warning(&mut data.warnings, warning);
        }
        append_unique_warning(
            &mut data.warnings,
            format!("Visual enrichment unavailable for {source_label}: {error}"),
        );
        if let Some(map) = data.document_map.as_mut() {
            fuse_document_map(map, &[], &data.warnings);
        }
        rebuild_document_ast_if_needed(data);
        return;
    }

    let enrichments = outcome.enrichments;
    if !enrichments.is_empty() {
        data.visual_enrichments = Some(Value::Array(enrichments.clone()));
    }

    for warning in outcome.warnings {
        append_unique_warning(&mut data.warnings, warning);
    }

    if let Some(map) = data.document_map.as_mut() {
        fuse_document_map(map, &enrichments, &data.warnings);
    }
    rebuild_document_ast_if_needed(data);
}

fn rebuild_document_ast_if_needed(data: &mut ReadPdfData) {
    let Some(context) = data.structured_fusion_context.clone() else {
        return;
    };
    if !context.emit_document_ast {
        return;
    }
    use crate::document_twin::{
        build_citation_chunks, build_document_ast, build_elements_with_tables_and_geometry,
    };
    let tables = if context.emit_tables {
        data.tables
            .clone()
            .unwrap_or_else(|| context.selectable_tables.clone())
    } else {
        context.selectable_tables.clone()
    };
    let semantic_elements = build_elements_with_tables_and_geometry(
        &context.pages,
        &tables,
        true,
        context.page_geometry.as_ref(),
    );
    let internal_chunks = build_citation_chunks(&semantic_elements, true);
    let warnings = data.warnings.clone().unwrap_or_default();
    let enrichments = data.visual_enrichments.clone().unwrap_or_else(|| json!([]));
    data.document_ast = Some(build_document_ast(
        &context.pages,
        &semantic_elements,
        &internal_chunks,
        &warnings,
        &enrichments,
    ));
}
