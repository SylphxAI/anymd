//! Shared conversion inputs, output sections, and diagnostics.

use std::path::Path;

use crate::images;

/// One citable unit of a converted document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Section {
    /// Human label used in page markers, e.g. "slide 3", "sheet Revenue", "chapter 2".
    pub label: String,
    /// Clean Markdown for this unit (no leading marker; the reader adds it).
    pub markdown: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Converted {
    /// Short format name: "docx", "pptx", "xlsx", "csv", "epub", "html", "image", "video".
    pub format: String,
    pub title: Option<String>,
    pub sections: Vec<Section>,
    /// Native table of contents: zero-based depth, title, 1-based section.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outline: Vec<(usize, String, Option<u32>)>,
    /// Small key/value facts worth a header line (author, dimensions, duration...).
    pub metadata: Vec<(String, String)>,
}

/// Admission for additional local files discovered while converting a source.
/// Implementations return the path to open, or refuse without reading its content.
pub trait SourceAccess: std::fmt::Debug + Send + Sync {
    fn admit_path(&self, path: &Path) -> Result<std::path::PathBuf, String>;
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Base URL for resolving relative links (HTML fetched from a URL).
    pub base_url: Option<String>,
    /// Opt-in OCR for images (runs a local `tesseract` binary when present).
    pub ocr: bool,
    /// Opt-in transcript for audio/video (runs the bundled transcribe-cpp Qwen3-ASR runtime).
    pub transcript: bool,
    /// With `transcript`: download the pinned Qwen3-ASR model into the
    /// anymd cache when none is installed (see [`crate::asr`]).
    pub download_asr_model: bool,
    /// Source path when the input came from disk (video/ffprobe needs a path).
    pub path: Option<std::path::PathBuf>,
    /// Export images embedded in DOCX, PPTX and EPUB files into this store and
    /// mark them in the Markdown. `None` keeps the alt-text-only form.
    pub images: Option<images::ImageStore>,
    /// How Word tracked changes and comments come out (DOCX).
    pub revisions: Revisions,
}

/// How a DOCX file's tracked changes and comments are written. A document
/// without any converts the same way under every choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Revisions {
    /// Tracked changes and comments as CriticMarkup (the default).
    #[default]
    Markup,
    /// The text with every change accepted; comments are left out.
    Accept,
    /// The text with every change rejected; comments are left out.
    Reject,
}

impl Revisions {
    /// `markup`, `accept` or `reject`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "markup" => Some(Self::Markup),
            "accept" => Some(Self::Accept),
            "reject" => Some(Self::Reject),
            _ => None,
        }
    }
}

/// A cumulative conversion ceiling which stopped a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionLimit {
    pub resource: &'static str,
    pub limit: usize,
}

impl std::fmt::Display for ConversionLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "conversion {} exceeds {} bytes",
            self.resource, self.limit
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// The bytes are not a valid instance of the format, or exceed its resource ceilings.
    Invalid(String),
    /// The format needs a tool or feature that is not available here.
    Unsupported(String),
}

impl ConvertError {
    pub(crate) fn limit(resource: &'static str, limit: usize) -> Self {
        Self::Invalid(ConversionLimit { resource, limit }.to_string())
    }

    /// Typed detail for a canonical resource-ceiling diagnostic.
    /// Existing exhaustive matches on this enum remain source-compatible.
    pub fn limit_exceeded(&self) -> Option<ConversionLimit> {
        let Self::Invalid(message) = self else {
            return None;
        };
        let rest = message.strip_prefix("conversion ")?;
        let (resource, limit) = rest.split_once(" exceeds ")?;
        let resource = match resource {
            "output" => "output",
            "inflated input" => "inflated input",
            _ => return None,
        };
        let detail = ConversionLimit {
            resource,
            limit: limit.strip_suffix(" bytes")?.parse().ok()?,
        };
        (detail.to_string() == *message).then_some(detail)
    }
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) | Self::Unsupported(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ConvertError {}
