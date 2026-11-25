//! Assemble the requested read_pdf fields and provider reconstruction context.

use serde_json::{json, Value};

use crate::document_twin::PageText;
use crate::text_index::PdfInfo;
use crate::{ENGINE_NAME, ENGINE_VERSION};

use super::super::pages::join_page_text;
use super::super::structured::{
    rebuild_structured_outputs, render_structured_html, render_structured_markdown,
};
use super::super::types::{EngineInfo, ReadPdfData, StructuredFusionContext, READ_PDF_ROUTE};
use super::{
    document_analysis::DocumentAnalysis, options::ExtractionOptions, page_content::PageContent,
    BuildSignals,
};

pub(super) struct PageSelection<'a> {
    pub(super) pages: &'a [PageText],
    pub(super) total_pages: u32,
    pub(super) explicit: bool,
}

pub(super) fn assemble_response(
    selection: PageSelection<'_>,
    options: &ExtractionOptions,
    pdf_info: Option<&PdfInfo>,
    signals: BuildSignals,
    content: PageContent,
    analysis: DocumentAnalysis,
) -> ReadPdfData {
    let pages = selection.pages;
    let empty_array = json!([]);
    let table_values = content.tables.as_ref().unwrap_or(&empty_array);
    let image_values = signals.images.as_ref().unwrap_or(&empty_array);
    let structured_fusion_context =
        fusion_context(&selection, options, &signals, &content, &analysis);

    let mut data = ReadPdfData {
        num_pages: options.page_count.then_some(selection.total_pages),
        info: options.metadata.then(|| pdf_info_json(pdf_info)),
        // TS/pdf.js only emits metadata when the catalog /Metadata stream exists.
        // Do not synthesize a metadata wrapper around info.
        metadata: (options.metadata && signals.has_catalog_metadata).then(|| json!({})),
        full_text: (!selection.explicit && options.full_text).then(|| join_page_text(pages)),
        page_texts: selection.explicit.then(|| pages.to_vec()),
        markdown: options
            .markdown
            .then(|| render_structured_markdown(pages, table_values, image_values)),
        html: options
            .html
            .then(|| render_structured_html(pages, table_values, image_values)),
        chunks: requested(options.chunks, content.chunks),
        elements: requested(options.elements || options.semantic_hints, content.elements),
        text_layer: requested(options.text_layer, content.text_layer),
        tables: requested(options.tables, content.tables),
        images: requested(
            options.images
                && image_values
                    .as_array()
                    .is_some_and(|images| !images.is_empty()),
            signals.images,
        ),
        safety_findings: requested(options.safety, analysis.safety),
        layout_diagnostics: requested(options.layout, analysis.layout),
        document_map: requested(options.document_map, analysis.document_map),
        document_ast: requested(options.document_ast, analysis.document_ast),
        trust_report: requested(options.trust, analysis.trust),
        accessibility_report: requested(options.accessibility, analysis.accessibility),
        outline: requested(options.outline, signals.outline),
        annotations: requested(options.annotations, signals.annotations),
        form_fields: requested(options.form_fields, signals.form_fields),
        attachments: requested(options.attachments, signals.attachments),
        structure_trees: requested(options.structure, signals.structure_trees),
        page_labels: requested(options.labels, signals.page_labels),
        page_geometry: requested(options.geometry, signals.page_geometry),
        permissions: requested(options.permissions, signals.permissions),
        mark_info: requested(options.permissions, signals.mark_info),
        ocr_text_layer: None,
        visual_enrichments: None,
        visual_enrichment_candidates: (options.visual
            && analysis
                .visual_candidates
                .as_array()
                .is_some_and(|values| !values.is_empty()))
        .then_some(analysis.visual_candidates),
        warnings: None,
        route: READ_PDF_ROUTE.into(),
        engine: EngineInfo {
            name: ENGINE_NAME,
            version: ENGINE_VERSION,
        },
        ocr_candidate_pages: if options.ocr {
            ocr_candidate_pages(pages)
        } else {
            Vec::new()
        },
        structured_fusion_context,
    };

    // The initial rebuild runs before the response warnings are attached.
    if let Some(context) = data.structured_fusion_context.clone() {
        rebuild_structured_outputs(&mut data, &context, &context.selectable_tables);
    }
    if !signals.warnings.is_empty() {
        data.warnings = Some(signals.warnings);
    }
    data
}

fn requested(want: bool, value: Option<Value>) -> Option<Value> {
    if want {
        value
    } else {
        None
    }
}

fn ocr_candidate_pages(pages: &[PageText]) -> Vec<u32> {
    let empty = pages
        .iter()
        .filter(|page| page.text.trim().is_empty())
        .map(|page| page.page)
        .collect::<Vec<_>>();
    if empty.is_empty() {
        pages.iter().map(|page| page.page).collect()
    } else {
        empty
    }
}

fn fusion_context(
    selection: &PageSelection<'_>,
    options: &ExtractionOptions,
    signals: &BuildSignals,
    content: &PageContent,
    analysis: &DocumentAnalysis,
) -> Option<StructuredFusionContext> {
    ((options.ocr
        && (options.tables
            || options.document_ast
            || options.document_map
            || options.visual
            || options.trust))
        || (options.visual && options.document_ast))
        .then(|| StructuredFusionContext {
            pages: selection.pages.to_vec(),
            total_pages: selection.total_pages,
            page_geometry: signals.page_geometry.clone(),
            selectable_tables: content.tables.clone().unwrap_or_else(|| json!([])),
            semantic_hints: options.semantic_hints,
            emit_markdown: options.markdown,
            emit_html: options.html,
            emit_chunks: options.chunks,
            emit_elements: options.elements || options.semantic_hints,
            emit_tables: options.tables,
            emit_document_ast: options.document_ast,
            emit_document_map: options.document_map,
            safety: analysis.safety.clone().unwrap_or_else(|| json!([])),
            layout: analysis.layout.clone().unwrap_or_else(|| json!([])),
            trust: analysis.trust.clone(),
            accessibility: analysis.accessibility.clone(),
            visual_candidates: analysis.visual_candidates.clone(),
        })
}

fn pdf_info_json(pdf_info: Option<&PdfInfo>) -> Value {
    // Match TS/pdf.js getMetadata().info without Rust-only fields.
    pdf_info
        .map(|pdf_info| {
            let mut values = serde_json::Map::new();
            // Match pdf.js getMetadata().info key order and flag presence.
            values.insert("PDFFormatVersion".into(), json!(pdf_info.format_version));
            values.insert(
                "Language".into(),
                pdf_info
                    .language
                    .as_ref()
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
            );
            values.insert(
                "EncryptFilterName".into(),
                pdf_info
                    .encrypt_filter_name
                    .as_ref()
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
            );
            values.insert("IsLinearized".into(), json!(pdf_info.is_linearized));
            values.insert(
                "IsAcroFormPresent".into(),
                json!(pdf_info.is_acroform_present),
            );
            values.insert("IsXFAPresent".into(), json!(pdf_info.is_xfa_present));
            values.insert(
                "IsCollectionPresent".into(),
                json!(pdf_info.is_collection_present),
            );
            values.insert(
                "IsSignaturesPresent".into(),
                json!(pdf_info.is_signatures_present),
            );
            for (key, value) in &pdf_info.fields {
                values.insert(key.clone(), json!(value));
            }
            Value::Object(values)
        })
        .unwrap_or_else(|| json!({}))
}
