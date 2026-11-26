use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{
    option_bool_schema, validate_source_count, validate_u32_min, validate_u32_range, PdfSource,
    ReadPdfAutoDetail, TrustReportRedaction,
};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct ReadPdfArgs {
    pub sources: Vec<PdfSource>,
    #[schemars(
        range(min = 500),
        description = "Token budget for the Markdown answer (default 20000). Longer documents stop at a page boundary and end with a cursor to continue."
    )]
    pub max_tokens: Option<u32>,
    #[schemars(
        description = "Continue a previous read: pass the cursor from its last line (\"<page>\" or \"<page>:<offset>\"). Single source only."
    )]
    pub cursor: Option<String>,
    #[schemars(
        description = "Legacy preset switch. true selects balanced (fast plus safety, trust, and accessibility) when profile and auto_detail are omitted. Omit it to use fast. false keeps manual control. auto_detail wins over profile. Never enables OCR."
    )]
    pub auto: Option<bool>,
    #[schemars(
        description = "Depth switch that wins over profile: fast, balanced, or full. full adds structure and audits. Never enables OCR or rendering."
    )]
    pub auto_detail: Option<ReadPdfAutoDetail>,
    #[schemars(
        description = "Named preset: fast (default), quality (structure, no audits, no OCR), or research (quality plus safety, trust, and accessibility). Ignored when auto is false or any include_* flag is set."
    )]
    pub profile: Option<String>,
    #[schemars(
        range(min = 1, max = 20),
        description = "Not used by read_pdf presets, which read every requested page. Sampling belongs to pdf_evidence inspect. Limit a read with sources[].pages."
    )]
    pub sample_pages: Option<u32>,
    pub include_full_text: Option<bool>,
    pub include_metadata: Option<bool>,
    pub include_page_count: Option<bool>,
    pub include_images: Option<bool>,
    pub include_tables: Option<bool>,
    pub include_elements: Option<bool>,
    pub include_semantic_hints: Option<bool>,
    pub include_markdown: Option<bool>,
    pub include_html: Option<bool>,
    pub include_chunks: Option<bool>,
    pub include_text_layer: Option<bool>,
    pub include_ocr_text_layer: Option<bool>,
    pub include_outline: Option<bool>,
    pub include_annotations: Option<bool>,
    pub include_page_labels: Option<bool>,
    pub include_page_geometry: Option<bool>,
    pub include_permissions: Option<bool>,
    pub include_form_fields: Option<bool>,
    pub include_attachments: Option<bool>,
    pub include_structure_tree: Option<bool>,
    pub include_safety_findings: Option<bool>,
    pub include_layout_diagnostics: Option<bool>,
    pub include_document_map: Option<bool>,
    pub include_document_ast: Option<bool>,
    pub include_visual_enrichments: Option<bool>,
    #[schemars(range(min = 1))]
    pub max_visual_enrichments: Option<u32>,
    pub include_trust_report: Option<bool>,
    pub trust_report_redaction: Option<TrustReportRedaction>,
    pub include_accessibility_report: Option<bool>,
}

impl ReadPdfArgs {
    pub fn validate(&self) -> Result<(), String> {
        validate_source_count(&self.sources)?;
        for source in &self.sources {
            source.validate()?;
        }
        validate_u32_range("sample_pages", self.sample_pages, 1, 20)?;
        validate_u32_min("max_tokens", self.max_tokens, 500)?;
        validate_u32_min("max_visual_enrichments", self.max_visual_enrichments, 1)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ComparePdfArgs {
    #[schemars(
        length(min = 1),
        description = "Local PDF path for the before document."
    )]
    pub before: String,
    #[schemars(
        length(min = 1),
        description = "Local PDF path for the after document."
    )]
    pub after: String,
    #[schemars(range(min = 1))]
    pub max_file_bytes: Option<u64>,
    #[schemars(range(min = 0, max = 1000))]
    pub context_chars: Option<u32>,
}

impl ComparePdfArgs {
    pub fn validate(&self) -> Result<(), String> {
        if self.before.trim().is_empty() || self.after.trim().is_empty() {
            return Err("before and after are required".into());
        }
        if self.before == self.after {
            return Err("before and after must be different PDFs".into());
        }
        if self.context_chars.is_some_and(|value| value > 1000) {
            return Err("context_chars must be <= 1000".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SearchPdfArgs {
    pub sources: Vec<PdfSource>,
    #[schemars(
        length(min = 1),
        description = "Literal text query to search for in extracted PDF text."
    )]
    pub query: String,
    pub case_sensitive: Option<bool>,
    pub whole_word: Option<bool>,
    pub include_ocr_text_layer: Option<bool>,
    #[schemars(range(min = 1, max = 1000))]
    pub max_pages: Option<u32>,
    #[schemars(range(min = 1, max = 500))]
    pub max_matches_per_source: Option<u32>,
    #[schemars(range(min = 0, max = 1000))]
    pub context_chars: Option<u32>,
    /// When true, match the current TS prefer_speed route: omit match geometry and
    /// emit the rust-text-index speed-route warning (ignored when OCR is enabled).
    #[schemars(
        description = "Prefer faster text-index search (omit match geometry; ignored when OCR is enabled).",
        schema_with = "option_bool_schema"
    )]
    #[serde(default)]
    pub prefer_speed: Option<bool>,
    #[schemars(
        description = "Return the detailed JSON result with match geometry and provenance instead of the compact Markdown list.",
        schema_with = "option_bool_schema"
    )]
    #[serde(default)]
    pub detail: Option<bool>,
}

impl SearchPdfArgs {
    pub fn validate(&self) -> Result<(), String> {
        validate_source_count(&self.sources)?;
        for source in &self.sources {
            source.validate()?;
        }
        if self.query.is_empty() {
            return Err("query must not be empty.".into());
        }
        validate_u32_range("max_pages", self.max_pages, 1, 1000)?;
        validate_u32_range(
            "max_matches_per_source",
            self.max_matches_per_source,
            1,
            500,
        )?;
        validate_u32_range("context_chars", self.context_chars, 0, 1000)
    }
}
