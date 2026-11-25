//! Request, response, and error contracts for read_pdf.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::text_index::{TextIndexError, TextIndexErrorCode};
use crate::HashError;

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
    pub(super) fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: ReadPdfErrorCode::InvalidParams,
            message: message.into(),
        }
    }

    pub(super) fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: ReadPdfErrorCode::InvalidRequest,
            message: message.into(),
        }
    }
}

impl crate::pages::PageSpecError for ReadPdfError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self::invalid_params(message)
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
