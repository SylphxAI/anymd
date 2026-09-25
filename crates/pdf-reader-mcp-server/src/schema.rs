//! Typed MCP tool argument schemas (schemars -> tools/list inputSchema).
//!
//! This module intentionally mirrors the immutable TypeScript v3.0.14 schemas.
//! `schemars` produces the client-visible contract; the `validate` methods close
//! the gap between descriptive JSON Schema and serde's structural decoding.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_SOURCES_PER_REQUEST: usize = 32;

fn option_bool_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    // Keep Option semantics (nullable) while guaranteeing a boolean type keyword for tools/list.
    serde_json::from_value(serde_json::json!({
        "type": ["boolean", "null"]
    }))
    .expect("option bool schema")
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct PageNumber(#[schemars(range(min = 1))] pub u32);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PageSpecifier {
    Pages(Vec<PageNumber>),
    Range(#[schemars(length(min = 1))] String),
}

impl PageSpecifier {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Pages(pages) if pages.iter().any(|page| page.0 == 0) => {
                Err("sources[].pages entries must be integers greater than or equal to 1.".into())
            }
            Self::Range(range) if range.is_empty() => {
                Err("sources[].pages range string must not be empty.".into())
            }
            _ => Ok(()),
        }
    }
}

// Client-visible tools/list schema intentionally omits oneOf/not exclusive
// constructs: OpenCode + Fireworks (and similar strict tool-schema validators)
// reject `{"required":["path"],"not":{"required":["url"]}}` (issue #562).
// Exactly-one-of path|url remains a hard runtime contract via `validate()`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PdfSource {
    #[schemars(
        length(min = 1),
        description = "Path to the local PDF file (absolute or relative to cwd). Provide exactly one of path or url (not both)."
    )]
    pub path: Option<String>,
    #[schemars(
        length(min = 1),
        description = "URL of the PDF file. Provide exactly one of path or url (not both)."
    )]
    pub url: Option<String>,
    pub pages: Option<PageSpecifier>,
}

impl PdfSource {
    pub fn validate(&self) -> Result<(), String> {
        let has_path = self.path.as_ref().is_some_and(|value| !value.is_empty());
        let has_url = self.url.as_ref().is_some_and(|value| !value.is_empty());
        match (has_path, has_url) {
            (true, false) | (false, true) => {}
            (false, false) | (true, true) => {
                return Err("Provide exactly one of path or url for each PDF source.".into());
            }
        }
        if let Some(pages) = &self.pages {
            pages.validate()?;
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        self.path
            .clone()
            .or_else(|| self.url.clone())
            .unwrap_or_else(|| "unknown".into())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadPdfAutoDetail {
    Fast,
    Balanced,
    Full,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrustReportRedaction {
    Standard,
    Strict,
    Off,
}

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
    #[schemars(description = "Depth switch that wins over profile: fast, balanced, or full. full adds structure and audits. Never enables OCR or rendering.")]
    pub auto_detail: Option<ReadPdfAutoDetail>,
    #[schemars(description = "Named preset: fast (default), quality (structure, no audits, no OCR), or research (quality plus safety, trust, and accessibility). Ignored when auto is false or any include_* flag is set.")]
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
    #[schemars(length(min = 1), description = "Local PDF path for the before document.")]
    pub before: String,
    #[schemars(length(min = 1), description = "Local PDF path for the after document.")]
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
    pub prefer_speed: Option<bool>,
    #[schemars(
        description = "Return the detailed JSON result with match geometry and provenance instead of the compact Markdown list.",
        schema_with = "option_bool_schema"
    )]
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

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RegionBoundingBox {
    #[schemars(range(min = 0.0))]
    pub left: f64,
    #[schemars(range(min = 0.0))]
    pub bottom: f64,
    #[schemars(range(min = 0.0))]
    pub right: f64,
    #[schemars(range(min = 0.0))]
    pub top: f64,
}

impl RegionBoundingBox {
    fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("left", self.left),
            ("bottom", self.bottom),
            ("right", self.right),
            ("top", self.top),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!(
                    "sources[].regions[].bounding_box.{name} must be at least 0."
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PdfEvidenceRegion {
    pub id: Option<String>,
    #[schemars(range(min = 1))]
    pub page: u32,
    pub bounding_box: RegionBoundingBox,
    #[schemars(range(min = 0.0, max = 200.0))]
    pub padding: Option<f64>,
}

impl PdfEvidenceRegion {
    fn validate(&self) -> Result<(), String> {
        validate_u32_min("sources[].regions[].page", Some(self.page), 1)?;
        self.bounding_box.validate()?;
        if let Some(padding) = self.padding {
            if !padding.is_finite() || !(0.0..=200.0).contains(&padding) {
                return Err("sources[].regions[].padding must be between 0 and 200.".into());
            }
        }
        Ok(())
    }
}

// Same provider-compat policy as PdfSource (#562): no oneOf/not in tools/list;
// exclusive path|url enforced at runtime by validate()/as_pdf_source().
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PdfEvidenceSource {
    #[schemars(
        length(min = 1),
        description = "Path to the local PDF file. Provide exactly one of path or url (not both)."
    )]
    pub path: Option<String>,
    #[schemars(
        length(min = 1),
        description = "URL of the PDF file. Provide exactly one of path or url (not both)."
    )]
    pub url: Option<String>,
    pub pages: Option<PageSpecifier>,
    pub regions: Option<Vec<PdfEvidenceRegion>>,
}

impl PdfEvidenceSource {
    pub fn validate(&self) -> Result<(), String> {
        let source = self.as_pdf_source();
        source.validate()?;
        if let Some(regions) = &self.regions {
            for region in regions {
                region.validate()?;
            }
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        self.path
            .clone()
            .or_else(|| self.url.clone())
            .unwrap_or_else(|| "unknown".into())
    }

    pub fn as_pdf_source(&self) -> PdfSource {
        PdfSource {
            path: self.path.clone(),
            url: self.url.clone(),
            pages: self.pages.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PdfEvidenceOperation {
    Inspect,
    RenderPage,
    ExtractRegions,
    OcrPages,
    AnalyzeRegions,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PdfEvidenceArgs {
    pub operation: PdfEvidenceOperation,
    pub sources: Vec<PdfEvidenceSource>,
    #[schemars(range(min = 1, max = 20))]
    pub sample_pages: Option<u32>,
    pub include_metadata: Option<bool>,
    #[schemars(range(min = 0.25, max = 4.0))]
    pub scale: Option<f64>,
    #[schemars(range(min = 1, max = 20))]
    pub max_pages: Option<u32>,
    #[schemars(range(min = 1, max = 100))]
    pub max_regions: Option<u32>,
    #[schemars(range(min = 10_000, max = 64_000_000))]
    pub max_pixels_per_page: Option<u64>,
    pub include_image: Option<bool>,
    #[schemars(range(min = 1_000, max = 300_000))]
    pub timeout_ms: Option<u32>,
    #[schemars(range(min = 1_000, max = 1_000_000))]
    pub max_output_chars: Option<u32>,
    pub languages: Option<Vec<String>>,
}

impl PdfEvidenceArgs {
    pub fn validate(&self) -> Result<(), String> {
        validate_source_count(&self.sources)?;
        for source in &self.sources {
            source.validate()?;
        }
        validate_u32_range("sample_pages", self.sample_pages, 1, 20)?;
        validate_f64_range("scale", self.scale, 0.25, 4.0)?;
        validate_u32_range("max_pages", self.max_pages, 1, 20)?;
        validate_u32_range("max_regions", self.max_regions, 1, 100)?;
        validate_u64_range(
            "max_pixels_per_page",
            self.max_pixels_per_page,
            10_000,
            64_000_000,
        )?;
        validate_u32_range("timeout_ms", self.timeout_ms, 1_000, 300_000)?;
        validate_u32_range("max_output_chars", self.max_output_chars, 1_000, 1_000_000)
    }
}

fn validate_source_count<T>(sources: &[T]) -> Result<(), String> {
    if sources.len() > MAX_SOURCES_PER_REQUEST {
        Err(format!(
            "Request accepts at most {MAX_SOURCES_PER_REQUEST} sources."
        ))
    } else {
        Ok(())
    }
}

fn validate_u32_min(name: &str, value: Option<u32>, min: u32) -> Result<(), String> {
    if value.is_some_and(|value| value < min) {
        Err(format!("{name} must be at least {min}."))
    } else {
        Ok(())
    }
}

fn validate_u32_range(name: &str, value: Option<u32>, min: u32, max: u32) -> Result<(), String> {
    if value.is_some_and(|value| !(min..=max).contains(&value)) {
        Err(format!("{name} must be between {min} and {max}."))
    } else {
        Ok(())
    }
}

fn validate_u64_range(name: &str, value: Option<u64>, min: u64, max: u64) -> Result<(), String> {
    if value.is_some_and(|value| !(min..=max).contains(&value)) {
        Err(format!("{name} must be between {min} and {max}."))
    } else {
        Ok(())
    }
}

fn validate_f64_range(name: &str, value: Option<f64>, min: f64, max: f64) -> Result<(), String> {
    if value.is_some_and(|value| !value.is_finite() || !(min..=max).contains(&value)) {
        Err(format!("{name} must be between {min} and {max}."))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{PdfEvidenceArgs, ReadPdfArgs, SearchPdfArgs};
    use serde::de::DeserializeOwned;
    use serde_json::Value;

    fn parse_and_validate<T>(value: Value, validate: impl FnOnce(&T) -> Result<(), String>) -> bool
    where
        T: DeserializeOwned,
    {
        serde_json::from_value::<T>(value)
            .ok()
            .is_some_and(|parsed| validate(&parsed).is_ok())
    }

    fn oracle() -> Value {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test/fixtures/legacy-tool-input-cases.json"
        )))
        .expect("parse legacy tool input cases")
    }

    fn accepts(tool: &str, args: Value) -> bool {
        match tool {
            "read_pdf" => parse_and_validate::<ReadPdfArgs>(args, ReadPdfArgs::validate),
            "search_pdf" => parse_and_validate::<SearchPdfArgs>(args, SearchPdfArgs::validate),
            "pdf_evidence" => {
                parse_and_validate::<PdfEvidenceArgs>(args, PdfEvidenceArgs::validate)
            }
            other => panic!("unknown oracle tool {other}"),
        }
    }

    #[test]
    fn immutable_v3_0_14_accept_reject_corpus_matches() {
        let oracle = oracle();
        assert_eq!(oracle["tag"], "v3.0.14");
        assert_eq!(oracle["commit"], "92651c79c6ce8d10dfa3c76332176c26f222bd78");
        for case in oracle["cases"].as_array().expect("oracle cases") {
            let id = case["id"].as_str().expect("case id");
            let tool = case["tool"].as_str().expect("case tool");
            let expected = case["accept"].as_bool().expect("case acceptance");
            let actual = accepts(tool, case["args"].clone());
            assert_eq!(actual, expected, "v3.0.14 contract case {id}");
        }
    }
}

#[cfg(test)]
mod provider_schema_compat_tests {
    use super::*;
    use schemars::schema_for;

    #[test]
    fn pdf_source_schema_omits_not_required_xor() {
        let schema = schema_for!(PdfSource);
        let json = serde_json::to_string(&schema).expect("serialize schema");
        assert!(
            !json.contains("\"not\""),
            "PdfSource tools/list schema must not emit not/required XOR for provider compat (#562): {json}"
        );
        assert!(
            !json.contains("\"oneOf\""),
            "PdfSource tools/list schema must not emit oneOf XOR for provider compat (#562): {json}"
        );
    }

    #[test]
    fn pdf_source_runtime_still_enforces_exactly_one_locator() {
        assert!(PdfSource {
            path: Some("a.pdf".into()),
            url: None,
            pages: None,
        }
        .validate()
        .is_ok());
        assert!(PdfSource {
            path: None,
            url: Some("https://example.com/a.pdf".into()),
            pages: None,
        }
        .validate()
        .is_ok());
        assert!(PdfSource {
            path: None,
            url: None,
            pages: None,
        }
        .validate()
        .is_err());
        assert!(PdfSource {
            path: Some("a.pdf".into()),
            url: Some("https://example.com/a.pdf".into()),
            pages: None,
        }
        .validate()
        .is_err());
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ReadArgs {
    #[schemars(
        length(min = 1),
        description = "File path, http(s) URL, or directory. PDF, DOCX, PPTX, XLSX/XLS/ODS, CSV/TSV, EPUB, HTML, Markdown/text, images, audio/video, SRT/VTT. A directory returns the list of readable files."
    )]
    pub source: String,
    #[schemars(
        description = "Pages (PDF), slides, sheets, or chapters to read, e.g. \"1-5,8\". Default: all."
    )]
    pub pages: Option<String>,
    #[schemars(
        range(min = 500),
        description = "Token budget (default 20000). Longer documents stop at a unit boundary and end with a cursor."
    )]
    pub max_tokens: Option<u32>,
    #[schemars(description = "Continue a previous read with the cursor from its last line.")]
    pub cursor: Option<String>,
    #[schemars(
        description = "OCR images and image-only PDF pages with a local tesseract. Default: automatic when tesseract is installed; false disables.",
        schema_with = "option_bool_schema"
    )]
    pub ocr: Option<bool>,
    #[schemars(
        description = "Transcribe audio/video with a local whisper.cpp (needs ANYMD_WHISPER_MODEL). Default false.",
        schema_with = "option_bool_schema"
    )]
    pub transcript: Option<bool>,
}

impl ReadArgs {
    pub fn validate(&self) -> Result<(), String> {
        if self.source.trim().is_empty() {
            return Err("source must not be empty.".into());
        }
        validate_u32_min("max_tokens", self.max_tokens, 500)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SearchArgs {
    #[schemars(length(min = 1), description = "Text to find.")]
    pub query: String,
    #[schemars(
        description = "Files, directories (searched recursively, .gitignore respected), or URLs. Default: the current directory."
    )]
    #[serde(default)]
    pub sources: Vec<String>,
    #[schemars(
        description = "auto (default): exact phrase, falling back to ranked passages when nothing matches. literal: exact phrase only. ranked: BM25 over the query words."
    )]
    pub mode: Option<String>,
    #[schemars(description = "Only search files matching this glob inside directories, e.g. \"*.pdf\" or \"reports/**\".")]
    pub glob: Option<String>,
    #[schemars(schema_with = "option_bool_schema")]
    pub case_sensitive: Option<bool>,
    #[schemars(schema_with = "option_bool_schema")]
    pub whole_word: Option<bool>,
    #[schemars(range(min = 1, max = 500), description = "Maximum hits to return (default 20).")]
    pub max_results: Option<u32>,
    #[schemars(range(min = 0, max = 1000), description = "Snippet context characters on each side (default 80).")]
    pub context_chars: Option<u32>,
}

impl SearchArgs {
    pub fn validate(&self) -> Result<(), String> {
        if self.query.trim().is_empty() {
            return Err("query must not be empty.".into());
        }
        if self.sources.len() > 256 {
            return Err("sources accepts at most 256 entries.".into());
        }
        validate_u32_range("max_results", self.max_results, 1, 500)?;
        validate_u32_range("context_chars", self.context_chars, 0, 1000)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InspectOperation {
    /// Page count, metadata, and per-page facts.
    Inspect,
    /// Render pages to PNG images.
    RenderPage,
    /// Crop regions (bounding boxes) out of rendered pages.
    ExtractRegions,
    /// OCR pages with the configured provider.
    OcrPages,
    /// Send regions to a configured vision/region provider.
    AnalyzeRegions,
    /// Structured JSON read: document map, elements, geometry, trust and accessibility reports.
    Structure,
    /// Page-level text diff between two PDFs (sources[0] = before, sources[1] = after).
    Compare,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct InspectArgs {
    pub operation: InspectOperation,
    pub sources: Vec<PdfEvidenceSource>,
    #[schemars(description = "structure only: fast (default), quality, or research (adds safety, trust, accessibility).")]
    pub profile: Option<String>,
    #[schemars(range(min = 1, max = 20))]
    pub sample_pages: Option<u32>,
    pub include_metadata: Option<bool>,
    #[schemars(range(min = 0.25, max = 4.0))]
    pub scale: Option<f64>,
    #[schemars(range(min = 1, max = 20))]
    pub max_pages: Option<u32>,
    #[schemars(range(min = 1, max = 100))]
    pub max_regions: Option<u32>,
    #[schemars(range(min = 10_000, max = 64_000_000))]
    pub max_pixels_per_page: Option<u64>,
    pub include_image: Option<bool>,
    #[schemars(range(min = 1_000, max = 300_000))]
    pub timeout_ms: Option<u32>,
    #[schemars(range(min = 1_000, max = 1_000_000))]
    pub max_output_chars: Option<u32>,
    pub languages: Option<Vec<String>>,
}
