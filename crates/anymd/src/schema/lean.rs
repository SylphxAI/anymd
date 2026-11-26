use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{
    option_bool_schema, option_images_schema, option_revisions_schema, validate_u32_min,
    validate_u32_range,
};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ReadArgs {
    #[schemars(
        length(min = 1),
        description = "File path, http(s) URL, or directory. PDF, DOCX, PPTX, XLSX/XLS/ODS, CSV/TSV, EPUB, HTML, Markdown/text, images, audio/video, SRT/VTT. A directory returns the list of readable files."
    )]
    pub source: String,
    #[schemars(
        description = "Local video only: read this bounded playback window as deterministic timeline sections. end_ms is required; the window is half-open, at most ten minutes and twenty scenes."
    )]
    pub timeline: Option<crate::video_request::TimelineSelection>,
    /// Read a section id returned by outline; repeat node with a continuation cursor.
    pub node: Option<String>,
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
        description = "Local OCR: auto, vlm, or tesseract. Auto uses the installed doc-VLM engine and weights (`anymd setup ocr` installs both, an explicit opt-in), otherwise tesseract. vlm without that setup returns a one-line setup notice. No automatic downloads. Boolean false disables; true retains automatic OCR."
    )]
    pub ocr: Option<crate::ocr_vlm::OcrSelection>,
    #[schemars(
        description = "Transcribe audio/video locally with bundled transcribe-cpp and Qwen3-ASR-1.7B Q8. Needs preinstalled SHA-256 pinned weights (~2.2 GB), unless download_asr_model is true. Audio stays local. Default false.",
        schema_with = "option_bool_schema"
    )]
    #[serde(default)]
    pub transcript: Option<bool>,
    #[schemars(
        description = "Implies transcript. Downloads pinned Qwen3-ASR-1.7B Q8 weights (~2.2 GB) into the anymd cache. Downloads only when explicitly enabled. Legacy download_whisper_model is accepted as an alias. Default false.",
        schema_with = "option_bool_schema"
    )]
    #[serde(default, alias = "download_whisper_model")]
    pub download_asr_model: Option<bool>,
    #[schemars(
        description = "Images embedded inside PDFs, DOCX, PPTX and EPUB files. \"refs\" (default) saves each meaningful raster image once to the anymd cache and marks its place in the Markdown as ![caption](absolute path) plus an <!-- image: WxH, page N --> comment, so you can open the file yourself; logos, icons and repeated headers are skipped. \"none\" leaves images out. Standalone image files are not affected.",
        schema_with = "option_images_schema"
    )]
    #[serde(default)]
    pub images: Option<String>,
    #[schemars(
        description = "Tracked changes and comments in Word (DOCX) files. \"markup\" (default) shows them as CriticMarkup: {++inserted++}, {--deleted--}, {~~old~>new~~}, {==commented==}{>>Author (date): comment<<}, each change followed by its author and date. \"accept\" gives the text with every change accepted and \"reject\" the text with every change rejected, both without comments. A document with no tracked changes or comments reads the same either way.",
        schema_with = "option_revisions_schema"
    )]
    #[serde(default)]
    pub revisions: Option<String>,
}

impl ReadArgs {
    /// Whether embedded images are exported (the default) or left out.
    pub fn wants_images(&self) -> bool {
        self.images.as_deref() != Some("none")
    }

    /// How Word tracked changes come out: CriticMarkup unless `accept` or
    /// `reject` was asked for.
    pub fn revisions(&self) -> anymd_formats::Revisions {
        self.revisions
            .as_deref()
            .and_then(anymd_formats::Revisions::parse)
            .unwrap_or_default()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.source.trim().is_empty() {
            return Err("source must not be empty.".into());
        }
        if let Some(mode) = self.revisions.as_deref() {
            if anymd_formats::Revisions::parse(mode).is_none() {
                return Err("revisions must be \"markup\", \"accept\" or \"reject\".".into());
            }
        }
        if let Some(mode) = self.images.as_deref() {
            if mode != "refs" && mode != "none" {
                return Err("images must be \"refs\" or \"none\".".into());
            }
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
    #[schemars(
        description = "Only search files matching this glob inside directories, e.g. \"*.pdf\" or \"reports/**\"."
    )]
    pub glob: Option<String>,
    #[schemars(schema_with = "option_bool_schema")]
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    #[schemars(schema_with = "option_bool_schema")]
    #[serde(default)]
    pub whole_word: Option<bool>,
    #[schemars(
        range(min = 1, max = 500),
        description = "Maximum hits to return (default 20)."
    )]
    pub max_results: Option<u32>,
    #[schemars(
        range(min = 0, max = 1000),
        description = "Snippet context characters on each side (default 80)."
    )]
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

/// Navigate a document without a model or vector index.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct OutlineArgs {
    #[schemars(
        description = "Local video only: outline this bounded playback window from metadata (cuts, chapters, subtitles). No ASR, OCR or captions run."
    )]
    pub timeline: Option<crate::video_request::TimelineSelection>,
    /// File path or http(s) URL.
    pub source: String,
    /// json (default) or tree.
    pub format: Option<String>,
    /// OCR image-only pages; false by default. Accepts booleans or auto/vlm/tesseract.
    pub ocr: Option<crate::ocr_vlm::OcrSelection>,
    /// Embedded images: none (default) or refs. Match this option in node reads.
    pub images: Option<String>,
    /// Word revisions: markup (default), accept or reject. Match node reads.
    pub revisions: Option<String>,
}
