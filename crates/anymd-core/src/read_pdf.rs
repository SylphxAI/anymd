//! Pure-Rust read_pdf for anymd (local path + SSRF-safe URL).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::text_index::{
    extract_pdf_text_from_document, PdfInfo, TextIndexError, TextIndexErrorCode,
};
use crate::url_fetch::{cleanup_temp_file, fetch_url_to_temp_file};
use crate::{HashError, ENGINE_NAME, ENGINE_VERSION};

mod pages;
mod presets;
mod structured;

use pages::*;
use presets::*;
pub(crate) use structured::*;

pub const READ_PDF_ROUTE: &str = "rust-read-pdf-v1";

#[derive(Debug, Clone, Deserialize)]
pub struct ReadPdfSource {
    pub path: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub pages: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ReadPdfInput {
    pub sources: Vec<ReadPdfSource>,
    pub include_metadata: bool,
    pub include_page_count: bool,
    pub include_full_text: bool,
    pub include_markdown: bool,
    pub include_chunks: bool,
    pub include_elements: bool,
    pub include_text_layer: bool,
    pub include_document_map: bool,
    pub auto: Option<bool>,
    pub auto_detail: Option<String>,
    pub profile: Option<String>,
    pub sample_pages: Option<u32>,
    pub include_images: bool,
    pub include_tables: bool,
    pub include_html: bool,
    pub include_semantic_hints: bool,
    pub include_outline: bool,
    pub include_annotations: bool,
    pub include_page_labels: bool,
    pub include_page_geometry: bool,
    pub include_permissions: bool,
    pub include_form_fields: bool,
    pub include_attachments: bool,
    pub include_structure_tree: bool,
    pub include_safety_findings: bool,
    pub include_layout_diagnostics: bool,
    pub include_document_ast: bool,
    pub include_ocr_text_layer: bool,
    pub include_visual_enrichments: bool,
    pub include_trust_report: bool,
    pub include_accessibility_report: bool,
    pub trust_report_redaction: Option<String>,
    pub max_visual_enrichments: Option<u32>,
    #[serde(skip)]
    pub(crate) auto_policy_resolved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineInfo {
    pub name: &'static str,
    pub version: &'static str,
}

/// Non-serialized source material needed to deterministically rebuild
/// table-derived surfaces after an optional OCR provider returns.
#[derive(Debug, Clone)]
pub struct StructuredFusionContext {
    pub pages: Vec<crate::document_twin::PageText>,
    pub total_pages: u32,
    pub page_geometry: Option<Value>,
    pub selectable_tables: Value,
    pub semantic_hints: bool,
    pub emit_markdown: bool,
    pub emit_html: bool,
    pub emit_chunks: bool,
    pub emit_elements: bool,
    pub emit_tables: bool,
    pub emit_document_ast: bool,
    pub emit_document_map: bool,
    pub safety: Value,
    pub layout: Value,
    pub trust: Option<Value>,
    pub accessibility: Option<Value>,
    pub visual_candidates: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ReadPdfData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_pages: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_texts: Option<Vec<crate::document_twin::PageText>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunks: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elements: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_layer: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_findings: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_diagnostics: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_map: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_ast: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_report: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessibility_report: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_fields: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_trees: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_labels: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_geometry: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark_info: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_text_layer: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_enrichments: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_enrichment_candidates: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    pub route: String,
    pub engine: EngineInfo,
    /// Selected OCR candidates used by the server-side provider boundary.
    #[serde(skip)]
    pub ocr_candidate_pages: Vec<u32>,
    /// Provider-neutral inputs for the post-OCR structured reconstruction pass.
    #[serde(skip)]
    pub structured_fusion_context: Option<StructuredFusionContext>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadPdfSourceResult {
    pub source: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ReadPdfData>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadPdfResponse {
    pub profile: &'static str,
    pub results: Vec<ReadPdfSourceResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadPdfErrorCode {
    InvalidParams,
    InvalidRequest,
    ExtractionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPdfError {
    pub code: ReadPdfErrorCode,
    pub message: String,
}

impl ReadPdfError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: ReadPdfErrorCode::InvalidParams,
            message: message.into(),
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: ReadPdfErrorCode::InvalidRequest,
            message: message.into(),
        }
    }
}

impl From<HashError> for ReadPdfError {
    fn from(error: HashError) -> Self {
        match error.code {
            crate::HashErrorCode::InvalidParams => Self::invalid_params(error.message),
            crate::HashErrorCode::InvalidRequest => Self::invalid_request(error.message),
        }
    }
}

impl From<TextIndexError> for ReadPdfError {
    fn from(error: TextIndexError) -> Self {
        match error.code {
            TextIndexErrorCode::InvalidParams => Self::invalid_params(error.message),
            TextIndexErrorCode::InvalidRequest => Self::invalid_request(error.message),
            TextIndexErrorCode::ExtractionFailed => Self {
                code: ReadPdfErrorCode::ExtractionFailed,
                message: error.message,
            },
        }
    }
}

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SELECTED_PAGES: usize = 10_001;

fn validate_source(source: &ReadPdfSource) -> Result<(), ReadPdfError> {
    let has_path = source.path.as_ref().is_some_and(|value| !value.is_empty());
    let has_url = source.url.as_ref().is_some_and(|value| !value.is_empty());
    match (has_path, has_url) {
        (true, false) | (false, true) => Ok(()),
        (false, false) => Err(ReadPdfError::invalid_params(
            "Provide exactly one of path or url for each PDF source.",
        )),
        (true, true) => Err(ReadPdfError::invalid_params(
            "Provide exactly one of path or url for each PDF source.",
        )),
    }
}

#[derive(Default)]
struct BuildSignals {
    page_geometry: Option<Value>,
    annotations: Option<Value>,
    page_labels: Option<Value>,
    permissions: Option<Value>,
    mark_info: Option<Value>,
    outline: Option<Value>,
    form_fields: Option<Value>,
    attachments: Option<Value>,
    structure_trees: Option<Value>,
    accessibility_structure_trees: Option<Value>,
    accessibility_structure_valid: bool,
    images: Option<Value>,
    /// Catalog /Metadata stream present. TS emits `metadata` only then.
    has_catalog_metadata: bool,
    warnings: Vec<String>,
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

fn auto_enabled(input: &ReadPdfInput) -> bool {
    if input.auto_policy_resolved {
        false
    } else {
        input.auto.unwrap_or_else(|| !has_any_include(input))
    }
}

fn requires_text_extraction(input: &ReadPdfInput) -> bool {
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

fn build_data(
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

fn read_source(source: &ReadPdfSource, input: &ReadPdfInput) -> ReadPdfSourceResult {
    if let Err(error) = validate_source(source) {
        return ReadPdfSourceResult {
            source: source
                .path
                .clone()
                .or_else(|| source.url.clone())
                .unwrap_or_else(|| "unknown".into()),
            success: false,
            error: Some(error.message),
            data: None,
        };
    }

    if let Some(path) = source.path.as_ref().filter(|p| !p.is_empty()) {
        let path_buf = PathBuf::from(path);
        return match read_local_pdf_filtered(path_buf.as_path(), input, &source.pages, path) {
            Ok(result) => result,
            Err(error) => ReadPdfSourceResult {
                source: path.clone(),
                success: false,
                error: Some(error.message),
                data: None,
            },
        };
    }

    if let Some(url) = source.url.as_ref().filter(|u| !u.is_empty()) {
        match fetch_url_to_temp_file(url) {
            Ok(temp) => {
                let result = read_local_pdf_filtered(temp.as_path(), input, &source.pages, url);
                cleanup_temp_file(temp.as_path());
                match result {
                    Ok(mut ok) => {
                        ok.source = url.clone();
                        ok
                    }
                    Err(error) => ReadPdfSourceResult {
                        source: url.clone(),
                        success: false,
                        error: Some(error.message),
                        data: None,
                    },
                }
            }
            Err(message) => ReadPdfSourceResult {
                source: url.clone(),
                success: false,
                error: Some(message),
                data: None,
            },
        }
    } else {
        ReadPdfSourceResult {
            source: "unknown".into(),
            success: false,
            error: Some("Provide exactly one of path or url for each PDF source.".into()),
            data: None,
        }
    }
}

fn read_local_pdf_filtered(
    path: &Path,
    input: &ReadPdfInput,
    pages_spec: &Option<Value>,
    source_label: &str,
) -> Result<ReadPdfSourceResult, ReadPdfError> {
    if let Some(mut cached) =
        crate::read_result_cache::get_cached_local_result(path, input, pages_spec)
    {
        // Preserve the caller's source label (path or original display form).
        cached.source = source_label.to_string();
        return Ok(cached);
    }
    let parsed = crate::cos_document::ParsedPdf::load(path, DEFAULT_MAX_FILE_BYTES)?;
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
    let explicit_pages = parse_page_spec(pages_spec)?;
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
        &selected_page_numbers,
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
            &selected_page_numbers,
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
            &selected_page_numbers,
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
    let data = build_data(
        &selected,
        total_pages,
        input,
        Some(&pdf_info),
        explicit_pages.is_some(),
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
        },
    );
    let result = ReadPdfSourceResult {
        source: source_label.to_string(),
        success: true,
        error: None,
        data: Some(data),
    };
    crate::read_result_cache::store_cached_local_result(path, input, pages_spec, &result);
    Ok(result)
}

pub fn read_pdf(input: &ReadPdfInput) -> Result<ReadPdfResponse, ReadPdfError> {
    if input.sources.is_empty() {
        return Err(ReadPdfError::invalid_params(
            "sources must include at least one PDF source.",
        ));
    }

    if let Some(profile) = input.profile.as_deref() {
        if !matches!(profile, "fast" | "quality" | "research") {
            return Err(ReadPdfError::invalid_params(
                "profile must be one of: fast, quality, research",
            ));
        }
    }
    if let Some(detail) = input.auto_detail.as_deref() {
        if !matches!(detail, "fast" | "balanced" | "full") {
            return Err(ReadPdfError::invalid_params(
                "auto_detail must be one of: fast, balanced, full",
            ));
        }
    }
    if let Some(sample) = input.sample_pages {
        if !(1..=20).contains(&sample) {
            return Err(ReadPdfError::invalid_params(
                "sample_pages must be an integer between 1 and 20",
            ));
        }
    }
    if let Some(max_vis) = input.max_visual_enrichments {
        if max_vis < 1 {
            return Err(ReadPdfError::invalid_params(
                "max_visual_enrichments must be >= 1 when provided",
            ));
        }
    }
    for source in &input.sources {
        parse_page_spec(&source.pages)?;
    }

    let mut results = Vec::new();
    for source in &input.sources {
        results.push(read_source(source, input));
    }

    if results.iter().all(|result| !result.success) {
        let errors = results
            .iter()
            .filter_map(|result| result.error.as_deref())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ReadPdfError::invalid_request(format!(
            "All PDF sources failed to process: {errors}"
        )));
    }

    Ok(ReadPdfResponse {
        profile: "pdf_read_results",
        results,
    })
}

pub fn read_pdf_from_value(input: &Value) -> Result<ReadPdfResponse, ReadPdfError> {
    let mut parsed: ReadPdfInput = serde_json::from_value(input.clone()).map_err(|error| {
        ReadPdfError::invalid_params(format!("Invalid read_pdf input: {error}"))
    })?;

    // Metadata and page count stay on unless the caller turns them off.
    if input.get("include_metadata").is_none() {
        parsed.include_metadata = true;
    }
    if input.get("include_page_count").is_none() {
        parsed.include_page_count = true;
    }

    let auto_specified = input.get("auto").is_some();
    let legacy_auto = parsed.auto == Some(true);
    let manual = (auto_specified && !legacy_auto)
        || (!auto_specified && json_has_explicit_read_options(input));

    if manual {
        if !auto_specified {
            parsed.auto = Some(false);
        }
    } else {
        let preset = resolved_read_preset(&parsed, legacy_auto);
        apply_read_preset(&mut parsed, input, &preset);
        // Resolved presets read every requested page. Sampling belongs to
        // pdf_evidence inspect, not this path.
        parsed.auto_policy_resolved = true;
    }

    read_pdf(&parsed)
}

#[cfg(test)]
mod tests;

