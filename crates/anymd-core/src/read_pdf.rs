//! Pure-Rust read_pdf for pdf-reader-mcp (local path + SSRF-safe URL).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::text_index::{
    extract_pdf_text_from_document, PdfInfo, TextIndexError, TextIndexErrorCode,
};
use crate::url_fetch::{cleanup_temp_file, fetch_url_to_temp_file};
use crate::{HashError, ENGINE_NAME, ENGINE_VERSION};

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

fn join_page_text(pages: &[crate::document_twin::PageText]) -> String {
    pages
        .iter()
        .map(|page| page.text.as_str())
        .filter(|page| !page.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn parse_page_spec(pages_spec: &Option<Value>) -> Result<Option<Vec<u32>>, ReadPdfError> {
    let Some(spec) = pages_spec else {
        return Ok(None);
    };
    let mut wanted = Vec::new();
    if let Some(arr) = spec.as_array() {
        if arr.is_empty() {
            return Err(ReadPdfError::invalid_params(
                "Page specification resulted in an empty set of pages.",
            ));
        }
        for value in arr {
            let page = value
                .as_u64()
                .filter(|page| *page > 0 && *page <= u64::from(u32::MAX));
            let Some(page) = page else {
                return Err(ReadPdfError::invalid_params(
                    "Page numbers in array must be positive integers.",
                ));
            };
            wanted.push(page as u32);
            if wanted.len() > MAX_SELECTED_PAGES {
                return Err(ReadPdfError::invalid_params(
                    "Page specification exceeds the maximum of 10001 selected pages.",
                ));
            }
        }
    } else if let Some(ranges) = spec.as_str() {
        if ranges.is_empty() {
            return Err(ReadPdfError::invalid_params("Invalid page number: "));
        }
        for raw_part in ranges.split(',') {
            let part = raw_part.trim();
            if let Some((start_text, end_text)) = part.split_once('-') {
                let start = parse_ts_positive_page(start_text);
                let end = if end_text.trim().is_empty() {
                    start.map(|page| page.saturating_add(10_000))
                } else {
                    parse_ts_positive_page(end_text)
                };
                let (Some(start), Some(end)) = (start, end) else {
                    return Err(ReadPdfError::invalid_params(format!(
                        "Invalid page range values: {part}"
                    )));
                };
                if start > end {
                    return Err(ReadPdfError::invalid_params(format!(
                        "Invalid page range values: {part}"
                    )));
                }
                wanted.extend(start..=end.min(start.saturating_add(10_000)));
            } else {
                let Some(page) = parse_ts_positive_page(part) else {
                    return Err(ReadPdfError::invalid_params(format!(
                        "Invalid page number: {part}"
                    )));
                };
                wanted.push(page);
            }
            if wanted.len() > MAX_SELECTED_PAGES {
                return Err(ReadPdfError::invalid_params(
                    "Page specification exceeds the maximum of 10001 selected pages.",
                ));
            }
        }
    } else {
        return Err(ReadPdfError::invalid_params(
            "Page specification must be a non-empty range string or array of positive integers.",
        ));
    }
    wanted.sort_unstable();
    wanted.dedup();
    if wanted.is_empty() {
        return Err(ReadPdfError::invalid_params(
            "Page specification resulted in an empty set of pages.",
        ));
    }
    Ok(Some(wanted))
}

fn parse_ts_positive_page(value: &str) -> Option<u32> {
    let value = value.trim_start();
    let value = value.strip_prefix('+').unwrap_or(value);
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty())
        .then(|| digits.parse::<u32>().ok())
        .flatten()
        .filter(|page| *page > 0)
}

fn evenly_sample_pages(total_pages: u32, max_samples: u32) -> Vec<u32> {
    let max_samples = max_samples.clamp(1, 20).min(total_pages.max(1));
    if total_pages <= max_samples {
        return (1..=total_pages).collect();
    }
    if max_samples == 1 {
        return vec![1];
    }
    let mut selected = Vec::with_capacity(max_samples as usize);
    for index in 0..max_samples {
        let numerator = u64::from(index) * u64::from(total_pages - 1);
        let denominator = u64::from(max_samples - 1);
        // Math.round for non-negative values.
        let offset = (numerator + denominator / 2) / denominator;
        selected.push(1 + offset as u32);
    }
    selected.sort_unstable();
    selected.dedup();
    selected
}

fn select_pages(
    pages: &[crate::text_index::ExtractedPageText],
    requested_pages: Option<&[u32]>,
) -> (Vec<crate::document_twin::PageText>, Vec<u32>) {
    let all: Vec<crate::document_twin::PageText> = pages
        .iter()
        .enumerate()
        .map(|(i, extracted)| crate::document_twin::PageText {
            page: (i + 1) as u32,
            text: extracted.text.clone(),
            positioned_items: extracted.positioned_items.clone(),
        })
        .collect();
    let Some(wanted) = requested_pages else {
        return (all, Vec::new());
    };
    let total_pages = pages.len() as u32;
    let invalid = wanted
        .iter()
        .copied()
        .filter(|page| *page > total_pages)
        .collect();
    let selected = all
        .into_iter()
        .filter(|page| wanted.binary_search(&page.page).is_ok())
        .collect();
    (selected, invalid)
}

fn render_structured_markdown(
    pages: &[crate::document_twin::PageText],
    tables: &Value,
    images: &Value,
) -> String {
    let mut parts = pages
        .iter()
        .map(|page| {
            let items = if page.positioned_items.is_empty() {
                page.text
                    .lines()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            } else {
                page.positioned_items
                    .iter()
                    .map(|item| item.text.trim())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            };
            let mut lines = vec![format!("## Page {}", page.page), String::new()];
            for item in items {
                lines.push(item.to_string());
                lines.push(String::new());
            }
            for image in images.as_array().into_iter().flatten().filter(|image| {
                image.get("page").and_then(Value::as_u64) == Some(u64::from(page.page))
            }) {
                let index = image.get("index").and_then(Value::as_u64).unwrap_or(0) + 1;
                let width = image.get("width").and_then(Value::as_u64).unwrap_or(0);
                let height = image.get("height").and_then(Value::as_u64).unwrap_or(0);
                let format = image.get("format").and_then(Value::as_str).unwrap_or("");
                lines.push(format!("[Image {index}: {width}x{height} {format}]"));
                lines.push(String::new());
            }
            lines.join("\n").trim_end().to_string()
        })
        .collect::<Vec<_>>();
    let table_values = tables.as_array().into_iter().flatten().collect::<Vec<_>>();
    if !table_values.is_empty() {
        let mut table_parts = vec!["## Extracted Tables".to_string(), String::new()];
        for table in table_values {
            let page = table.get("page").and_then(Value::as_u64).unwrap_or(0);
            let index = table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0) + 1;
            let confidence = table
                .get("confidence")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            table_parts.push(format!("### Page {page}, Table {index}"));
            table_parts.push(format!("*Confidence: {:.0}%*", confidence * 100.0));
            table_parts.push(String::new());
            let mut lines = Vec::new();
            for (row_index, row) in table
                .get("rows")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let cells = row
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|cell| {
                        let trimmed = cell.as_str().unwrap_or("").trim();
                        if trimmed.is_empty() {
                            " ".to_string()
                        } else {
                            trimmed.to_string()
                        }
                    })
                    .collect::<Vec<_>>();
                lines.push(format!("| {} |", cells.join(" | ")));
                if row_index == 0 {
                    lines.push(format!(
                        "| {} |",
                        cells.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")
                    ));
                }
            }
            if !lines.is_empty() {
                table_parts.push(lines.join("\n"));
                table_parts.push(String::new());
            }
        }
        parts.push(table_parts.join("\n"));
    }
    parts.join("\n\n").trim().to_string()
}

fn render_structured_html(
    pages: &[crate::document_twin::PageText],
    tables: &Value,
    images: &Value,
) -> String {
    let mut parts = pages
        .iter()
        .map(|page| {
            let items = if page.positioned_items.is_empty() {
                page.text
                    .lines()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            } else {
                page.positioned_items
                    .iter()
                    .map(|item| item.text.trim())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            };
            let mut body = vec![
                format!("<section data-page=\"{}\">", page.page),
                format!("<h2>Page {}</h2>", page.page),
            ];
            body.extend(
                items
                    .into_iter()
                    .map(|item| format!("<p>{}</p>", html_escape(item))),
            );
            for image in images.as_array().into_iter().flatten().filter(|image| {
                image.get("page").and_then(Value::as_u64) == Some(u64::from(page.page))
            }) {
                let index = image.get("index").and_then(Value::as_u64).unwrap_or(0);
                let width = image.get("width").and_then(Value::as_u64).unwrap_or(0);
                let height = image.get("height").and_then(Value::as_u64).unwrap_or(0);
                let format = html_escape(image.get("format").and_then(Value::as_str).unwrap_or(""));
                body.push(format!(
                    "<figure data-image-index=\"{index}\">\n<figcaption>Image {}: {width}x{height} {format}</figcaption>\n</figure>",
                    index + 1
                ));
            }
            body.push("</section>".into());
            body.join("\n")
        })
        .collect::<Vec<_>>();
    for table in tables.as_array().into_iter().flatten() {
        let rows = table
            .get("rows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|row| {
                let cells = row
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|cell| format!("<td>{}</td>", html_escape(cell.as_str().unwrap_or(""))))
                    .collect::<String>();
                format!("<tr>{cells}</tr>")
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!(
            "<table data-page=\"{}\" data-table-index=\"{}\">\n<tbody>\n{rows}\n</tbody>\n</table>",
            table.get("page").and_then(Value::as_u64).unwrap_or(0),
            table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0)
        ));
    }
    parts.join("\n\n").trim().to_string()
}

pub(crate) fn rebuild_structured_outputs(
    data: &mut ReadPdfData,
    context: &StructuredFusionContext,
    tables: &Value,
) {
    use crate::document_twin::{
        build_citation_chunks, build_document_ast, build_document_map,
        build_elements_with_tables_and_geometry,
    };

    let output_elements = build_elements_with_tables_and_geometry(
        &context.pages,
        tables,
        context.semantic_hints,
        context.page_geometry.as_ref(),
    );
    let semantic_elements = if context.semantic_hints {
        output_elements.clone()
    } else {
        build_elements_with_tables_and_geometry(
            &context.pages,
            tables,
            true,
            context.page_geometry.as_ref(),
        )
    };
    let output_chunks = build_citation_chunks(&output_elements, context.semantic_hints);
    let internal_chunks = if context.emit_chunks {
        output_chunks.clone()
    } else {
        build_citation_chunks(&semantic_elements, true)
    };
    let ast_warnings = data.warnings.clone().unwrap_or_default();
    let text_layer = crate::document_twin::build_text_layer(&context.pages);
    let images = data.images.clone().unwrap_or_else(|| json!([]));
    if context.emit_markdown {
        data.markdown = Some(render_structured_markdown(&context.pages, tables, &images));
    }
    if context.emit_html {
        data.html = Some(render_structured_html(&context.pages, tables, &images));
    }
    if context.emit_chunks {
        data.chunks = Some(output_chunks.clone());
    }
    if context.emit_elements {
        data.elements = Some(output_elements.clone());
    }
    if context.emit_tables {
        data.tables = Some(tables.clone());
    }
    if context.emit_document_ast {
        let visual_enrichments = data.visual_enrichments.clone().unwrap_or_else(|| json!([]));
        data.document_ast = Some(build_document_ast(
            &context.pages,
            &semantic_elements,
            &internal_chunks,
            &ast_warnings,
            &visual_enrichments,
        ));
    }
    if context.emit_document_map {
        data.document_map = Some(build_document_map(
            &context.pages,
            context.total_pages,
            &semantic_elements,
            &internal_chunks,
            &context.safety,
            &context.layout,
            &text_layer,
            context.page_geometry.as_ref(),
            &ast_warnings,
            context.trust.as_ref(),
            context.accessibility.as_ref(),
            &context.visual_candidates,
        ));
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

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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

fn json_has_explicit_read_options(input: &Value) -> bool {
    const KEYS: &[&str] = &[
        "include_full_text",
        "include_metadata",
        "include_page_count",
        "include_images",
        "include_tables",
        "include_elements",
        "include_semantic_hints",
        "include_markdown",
        "include_html",
        "include_chunks",
        "include_text_layer",
        "include_ocr_text_layer",
        "include_outline",
        "include_annotations",
        "include_page_labels",
        "include_page_geometry",
        "include_permissions",
        "include_form_fields",
        "include_attachments",
        "include_structure_tree",
        "include_safety_findings",
        "include_layout_diagnostics",
        "include_document_map",
        "include_document_ast",
        "include_visual_enrichments",
        "max_visual_enrichments",
        "include_trust_report",
        "trust_report_redaction",
        "include_accessibility_report",
    ];
    // `pages` is a filter, not a mode switch. A null pages key is how the MCP
    // server materializes "not specified"; a real page list still uses the preset.
    KEYS.iter().any(|key| input.get(*key).is_some())
}

fn enable_absent(input: &Value, key: &str, field: &mut bool) {
    if input.get(key).is_none() {
        *field = true;
    }
}

/// Named read presets. OCR, rendering, and visual analysis are never included.
fn apply_read_preset(parsed: &mut ReadPdfInput, input: &Value, preset: &str) {
    let structural = matches!(preset, "quality" | "research" | "full");
    let audits = matches!(preset, "balanced" | "research" | "full");
    let known = matches!(preset, "fast" | "balanced" | "quality" | "research" | "full");
    if !known {
        return;
    }
    enable_absent(input, "include_metadata", &mut parsed.include_metadata);
    enable_absent(input, "include_page_count", &mut parsed.include_page_count);
    enable_absent(
        input,
        "include_page_geometry",
        &mut parsed.include_page_geometry,
    );
    enable_absent(input, "include_document_map", &mut parsed.include_document_map);
    enable_absent(input, "include_chunks", &mut parsed.include_chunks);
    enable_absent(input, "include_markdown", &mut parsed.include_markdown);
    enable_absent(input, "include_tables", &mut parsed.include_tables);
    enable_absent(
        input,
        "include_semantic_hints",
        &mut parsed.include_semantic_hints,
    );
    enable_absent(
        input,
        "include_layout_diagnostics",
        &mut parsed.include_layout_diagnostics,
    );
    if structural {
        enable_absent(input, "include_full_text", &mut parsed.include_full_text);
        enable_absent(input, "include_html", &mut parsed.include_html);
        enable_absent(input, "include_elements", &mut parsed.include_elements);
        enable_absent(input, "include_text_layer", &mut parsed.include_text_layer);
        enable_absent(
            input,
            "include_document_ast",
            &mut parsed.include_document_ast,
        );
        enable_absent(input, "include_outline", &mut parsed.include_outline);
        enable_absent(input, "include_annotations", &mut parsed.include_annotations);
        enable_absent(input, "include_page_labels", &mut parsed.include_page_labels);
        enable_absent(input, "include_permissions", &mut parsed.include_permissions);
        enable_absent(input, "include_form_fields", &mut parsed.include_form_fields);
        enable_absent(input, "include_attachments", &mut parsed.include_attachments);
        enable_absent(
            input,
            "include_structure_tree",
            &mut parsed.include_structure_tree,
        );
    }
    if audits {
        enable_absent(
            input,
            "include_safety_findings",
            &mut parsed.include_safety_findings,
        );
        enable_absent(input, "include_trust_report", &mut parsed.include_trust_report);
        enable_absent(
            input,
            "include_accessibility_report",
            &mut parsed.include_accessibility_report,
        );
    }
}

fn resolved_read_preset(parsed: &ReadPdfInput, legacy_auto: bool) -> String {
    // `auto_detail` is the legacy depth switch and wins over `profile`.
    if let Some(detail) = parsed.auto_detail.as_deref() {
        return detail.to_string();
    }
    if let Some(profile) = parsed.profile.as_deref() {
        return profile.to_string();
    }
    if legacy_auto {
        "balanced".to_string()
    } else {
        "fast".to_string()
    }
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

