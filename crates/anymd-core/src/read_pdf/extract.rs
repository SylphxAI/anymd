//! Extraction policy and construction of read_pdf output from selected pages and PDF signals.

use serde_json::{json, Value};

use crate::text_index::PdfInfo;
use crate::{ENGINE_NAME, ENGINE_VERSION};

use super::pages::join_page_text;
use super::structured::{
    rebuild_structured_outputs, render_structured_html, render_structured_markdown,
};
use super::types::{
    EngineInfo, ReadPdfData, ReadPdfInput, StructuredFusionContext, READ_PDF_ROUTE,
};

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

pub(super) fn auto_enabled(input: &ReadPdfInput) -> bool {
    if input.auto_policy_resolved {
        false
    } else {
        input.auto.unwrap_or_else(|| !has_any_include(input))
    }
}

pub(super) fn requires_text_extraction(input: &ReadPdfInput) -> bool {
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

pub(super) fn build_data(
    pages: &[crate::document_twin::PageText],
    total_pages: u32,
    input: &ReadPdfInput,
    pdf_info: Option<&PdfInfo>,
    explicit_page_selection: bool,
    signals: BuildSignals,
) -> ReadPdfData {
    use crate::document_twin::{
        build_citation_chunks, build_document_ast, build_document_map, build_layout_diagnostics,
        build_safety_findings, build_tables_with_admission, build_text_layer, build_trust_report,
    };

    let BuildSignals {
        page_geometry,
        annotations,
        page_labels,
        permissions,
        mark_info,
        outline,
        form_fields,
        attachments,
        structure_trees,
        accessibility_structure_trees,
        accessibility_structure_valid,
        images,
        has_catalog_metadata,
        mut warnings,
    } = signals;
    let full_text = join_page_text(pages);

    // auto is resolved in read_pdf_from_value for JSON callers; programmatic callers
    // should set auto explicitly. Default remains true only when auto is None and no
    // include flags are set (bool defaults are false, so all-false means sources-only).
    // Presence of true include flags implies manual mode when auto is omitted.
    // (JSON path resolves preset flags before reaching this function.)
    let auto = auto_enabled(input);
    let detail = input.auto_detail.as_deref().unwrap_or("balanced");
    let auto_full = auto && detail == "full";
    let auto_balanced = auto && matches!(detail, "balanced" | "full");
    let auto_fast = auto;

    let want_meta = input.include_metadata || auto_fast;
    let want_page_count = input.include_page_count || auto_fast;
    let want_text = input.include_full_text || auto_full;
    let want_md = input.include_markdown || auto_fast;
    let want_chunks = input.include_chunks || auto_fast;
    let want_elements = input.include_elements || auto_full;
    let want_semantic = input.include_semantic_hints || auto_fast;
    let want_text_layer = input.include_text_layer || auto_full;
    let want_map = input.include_document_map || auto_fast;
    let want_tables = input.include_tables || auto_fast;
    let want_html = input.include_html || auto_full;
    let want_safety = input.include_safety_findings || auto_balanced || auto_full;
    let want_layout = input.include_layout_diagnostics || auto_fast;
    let want_ast = input.include_document_ast || auto_full;
    let want_trust = input.include_trust_report || auto_balanced || auto_full;
    let want_a11y = input.include_accessibility_report || auto_balanced || auto_full;
    let want_outline = input.include_outline || auto_full;
    let want_annotations = input.include_annotations || auto_full;
    let want_labels = input.include_page_labels || auto_full;
    let want_geometry = input.include_page_geometry || auto_fast;
    let want_permissions = input.include_permissions || auto_full;
    let want_forms = input.include_form_fields || auto_full;
    let want_attachments = input.include_attachments || auto_full;
    let want_structure = input.include_structure_tree || auto_full;
    let want_images = input.include_images;
    let want_ocr = input.include_ocr_text_layer;
    let want_visual = input.include_visual_enrichments;

    if want_ocr {
        warnings.push(crate::ocr_fusion::OCR_STUB_WARNING.into());
    }
    if want_visual {
        warnings
            .push("Visual enrichment skipped: analyze_regions provider is not_configured.".into());
    }

    let page_content_table_geometry = want_text
        || want_elements
        || want_semantic
        || want_md
        || want_html
        || want_chunks
        || want_text_layer
        || want_ocr
        || want_images
        || want_safety
        || want_layout
        || want_map
        || want_ast
        || want_visual
        || want_trust
        || want_a11y;
    let tables = if want_tables || want_ast || want_map || want_visual || want_trust {
        let (tables, table_warnings) =
            build_tables_with_admission(pages, page_content_table_geometry);
        warnings.extend(table_warnings);
        Some(tables)
    } else {
        None
    };
    let empty_array = json!([]);
    let table_values = tables.as_ref().unwrap_or(&empty_array);
    let image_values = images.as_ref().unwrap_or(&empty_array);
    let plain_elements = ((want_elements || want_chunks) && !want_semantic).then(|| {
        crate::document_twin::build_elements_with_tables_images_and_geometry(
            pages,
            table_values,
            image_values,
            false,
            page_geometry.as_ref(),
        )
    });
    let semantic_elements =
        (want_semantic || want_ast || want_map || want_visual || want_trust || want_a11y).then(
            || {
                crate::document_twin::build_elements_with_tables_images_and_geometry(
                    pages,
                    table_values,
                    image_values,
                    true,
                    page_geometry.as_ref(),
                )
            },
        );
    let elements = if want_elements || want_semantic {
        if want_semantic {
            semantic_elements.clone()
        } else {
            plain_elements.clone()
        }
    } else {
        None
    };
    let chunks = if want_chunks {
        let chunk_elements = if want_semantic {
            semantic_elements.as_ref()
        } else {
            plain_elements.as_ref()
        };
        Some(build_citation_chunks(
            chunk_elements.unwrap_or(&empty_array),
            want_semantic,
        ))
    } else {
        None
    };
    let internal_chunks = if want_ast || want_map {
        chunks.clone().or_else(|| {
            Some(build_citation_chunks(
                semantic_elements.as_ref().unwrap_or(&empty_array),
                true,
            ))
        })
    } else {
        None
    };
    let safety = if want_safety || want_trust || want_map {
        Some(build_safety_findings(pages))
    } else {
        None
    };
    let layout = if want_layout || want_trust || want_map {
        Some(build_layout_diagnostics(pages))
    } else {
        None
    };
    let text_layer = (want_text_layer || want_map).then(|| build_text_layer(pages));
    let redaction = input
        .trust_report_redaction
        .as_deref()
        .unwrap_or("standard");
    let trust = if want_trust {
        Some(build_trust_report(
            pages,
            safety.as_ref().unwrap_or(&json!([])),
            layout.as_ref().unwrap_or(&json!([])),
            semantic_elements.as_ref().unwrap_or(&empty_array),
            annotations.as_ref(),
            redaction,
        ))
    } else {
        None
    };
    let a11y = if want_a11y {
        Some(crate::accessibility::build_accessibility_report(
            crate::accessibility::AccessibilityInput {
                pages,
                elements: semantic_elements.as_ref().unwrap_or(&empty_array),
                structure_trees: accessibility_structure_trees.as_ref(),
                annotations: annotations.as_ref(),
                form_fields: form_fields.as_ref(),
                permissions: permissions.as_ref(),
                mark_info: mark_info.as_ref(),
                outline: outline.as_ref(),
                structure_valid: accessibility_structure_valid,
            },
        ))
    } else {
        None
    };

    let visual_candidates = if want_visual {
        let geometry = page_geometry.as_ref().and_then(Value::as_array);
        let outcome = crate::visual_candidates::select_visual_enrichment_candidates(
            semantic_elements
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

    let document_ast = if want_ast {
        Some(build_document_ast(
            pages,
            semantic_elements.as_ref().unwrap_or(&empty_array),
            internal_chunks.as_ref().unwrap_or(&empty_array),
            &warnings,
            &empty_array,
        ))
    } else {
        None
    };

    let document_map = if want_map {
        Some(build_document_map(
            pages,
            total_pages,
            semantic_elements.as_ref().unwrap_or(&empty_array),
            internal_chunks.as_ref().unwrap_or(&empty_array),
            safety.as_ref().unwrap_or(&json!([])),
            layout.as_ref().unwrap_or(&json!([])),
            text_layer.as_ref().unwrap_or(&empty_array),
            page_geometry.as_ref(),
            &warnings,
            trust.as_ref(),
            a11y.as_ref(),
            &visual_candidates,
        ))
    } else {
        None
    };

    let mut data = ReadPdfData {
        num_pages: want_page_count.then_some(total_pages),
        info: None,
        metadata: None,
        full_text: None,
        page_texts: None,
        markdown: None,
        html: None,
        chunks: None,
        elements: None,
        text_layer: None,
        tables: None,
        images: None,
        safety_findings: None,
        layout_diagnostics: None,
        document_map: None,
        document_ast: None,
        trust_report: None,
        accessibility_report: None,
        outline: None,
        annotations: None,
        form_fields: None,
        attachments: None,
        structure_trees: None,
        page_labels: None,
        page_geometry: None,
        permissions: None,
        mark_info: None,
        ocr_text_layer: None,
        visual_enrichments: None,
        visual_enrichment_candidates: None,
        warnings: None,
        route: READ_PDF_ROUTE.into(),
        engine: EngineInfo {
            name: ENGINE_NAME,
            version: ENGINE_VERSION,
        },
        ocr_candidate_pages: if want_ocr {
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
        } else {
            Vec::new()
        },
        structured_fusion_context: ((want_ocr
            && (want_tables || want_ast || want_map || want_visual || want_trust))
            || (want_visual && want_ast))
            .then(|| StructuredFusionContext {
                pages: pages.to_vec(),
                total_pages,
                page_geometry: page_geometry.clone(),
                selectable_tables: tables.clone().unwrap_or_else(|| json!([])),
                semantic_hints: want_semantic,
                emit_markdown: want_md,
                emit_html: want_html,
                emit_chunks: want_chunks,
                emit_elements: want_elements || want_semantic,
                emit_tables: want_tables,
                emit_document_ast: want_ast,
                emit_document_map: want_map,
                safety: safety.clone().unwrap_or_else(|| json!([])),
                layout: layout.clone().unwrap_or_else(|| json!([])),
                trust: trust.clone(),
                accessibility: a11y.clone(),
                visual_candidates: visual_candidates.clone(),
            }),
    };

    if want_meta {
        let info = pdf_info
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
            .unwrap_or_else(|| json!({}));
        // Match TS/pdf.js getMetadata().info: do not inject rust-only extras
        // (text_chars/route/num_pages). route and num_pages stay on data.*; text_chars
        // is not a public pdf.js info field.
        data.info = Some(info);
        // Match TS/pdf.js: `metadata` is only present when getMetadata() returns a
        // metadata object (catalog /Metadata stream). Do not invent a synthetic
        // wrapper around info. When the stream exists but exposes no
        // getAll keys (common in pdfjs-dist Node), TS returns {}.
        if has_catalog_metadata {
            data.metadata = Some(json!({}));
        }
    }
    if explicit_page_selection {
        data.page_texts = Some(pages.to_vec());
    } else if want_text {
        data.full_text = Some(full_text.clone());
    }
    if want_md {
        data.markdown = Some(render_structured_markdown(
            pages,
            tables.as_ref().unwrap_or(&empty_array),
            image_values,
        ));
    }
    if want_html {
        data.html = Some(render_structured_html(
            pages,
            tables.as_ref().unwrap_or(&empty_array),
            image_values,
        ));
    }
    if want_chunks {
        data.chunks = chunks.clone();
    }
    if want_elements || want_semantic {
        data.elements = elements.clone();
    }
    if want_text_layer {
        data.text_layer = text_layer;
    }
    if want_tables {
        data.tables = tables.clone();
    }
    if want_images
        && image_values
            .as_array()
            .is_some_and(|images| !images.is_empty())
    {
        data.images = Some(image_values.clone());
    }
    if want_safety {
        data.safety_findings = safety.clone();
    }
    if want_layout {
        data.layout_diagnostics = layout.clone();
    }
    if want_map {
        data.document_map = document_map;
    }
    if want_ast {
        data.document_ast = document_ast;
    }
    if want_trust {
        data.trust_report = trust;
    }
    if want_a11y {
        data.accessibility_report = a11y;
    }
    if want_outline {
        data.outline = outline;
    }
    if want_annotations {
        data.annotations = annotations;
    }
    if want_forms {
        data.form_fields = form_fields;
    }
    if want_attachments {
        data.attachments = attachments;
    }
    if want_structure {
        data.structure_trees = structure_trees;
    }
    if want_labels {
        data.page_labels = page_labels;
    }
    if want_geometry {
        data.page_geometry = page_geometry;
    }
    if want_permissions {
        data.permissions = permissions;
        data.mark_info = mark_info;
    }
    if want_visual
        && visual_candidates
            .as_array()
            .is_some_and(|values| !values.is_empty())
    {
        data.visual_enrichment_candidates = Some(visual_candidates);
    }

    if let Some(context) = data.structured_fusion_context.clone() {
        rebuild_structured_outputs(&mut data, &context, &context.selectable_tables);
    }

    if !warnings.is_empty() {
        data.warnings = Some(warnings);
    }
    data
}
