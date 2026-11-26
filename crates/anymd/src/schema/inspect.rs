use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::evidence::PdfEvidenceSource;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InspectOperation {
    /// Bounded local video timeline.
    VideoTimeline,
    /// Decode actual video frames at requested playback timestamps.
    RenderFrame,
    /// Deterministic quote and location support, not semantic truth.
    CiteCheck,
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

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CiteNormalization {
    #[default]
    None,
    WhitespaceV1,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CiteBoundingBox {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub id: Option<String>,
    #[schemars(
        length(min = 1, max = 4096),
        description = "Nonempty quote or direct read/search output snippet, at most 4096 UTF-16 units; request total at most 64000 units."
    )]
    pub quote: String,
    #[serde(default)]
    #[schemars(
        range(min = 1),
        description = "One-based physical PDF page, not a printed page label. Optional when quote includes a read `<!-- page N -->` marker or search `- p.N:` locator. At most 20 distinct pages per request."
    )]
    pub page: u32,
    /// Optional bottom-left PDF coordinates, finite and with positive area. Omit to verify the quote anywhere on the page and return its supporting boxes.
    pub bounding_box: Option<CiteBoundingBox>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct InspectArgs {
    #[schemars(
        length(min = 1, max = 100),
        description = "cite_check only: 1–100 quotes with required page and bounding_box; exactly one PDF source."
    )]
    pub citations: Option<Vec<Citation>>,
    pub normalization: Option<CiteNormalization>,
    pub operation: InspectOperation,
    #[schemars(
        description = "video_timeline only (required there): half-open playback window with required end_ms, at most ten minutes and twenty detected-cut scenes. caption only samples one frame per scene through a user-configured local-command adapter."
    )]
    pub timeline: Option<crate::video_request::TimelineSelection>,
    #[schemars(
        description = "render_frame only: playback timestamps in integer milliseconds (at most twenty). Returns decoded frame metadata with the actual decoded timestamp."
    )]
    pub timestamps_ms: Option<Vec<u64>>,
    #[schemars(
        length(min = 64, max = 64),
        description = "cite_check and video operations: fail if the admitted source's SHA-256 differs from this hexadecimal value."
    )]
    pub expected_source_sha256: Option<String>,
    #[schemars(
        description = "video_timeline only: also attach timed ASR cues from preinstalled local Qwen3-ASR weights. Never downloads models. Default false."
    )]
    pub transcript: Option<bool>,
    pub sources: Vec<PdfEvidenceSource>,
    #[schemars(
        description = "structure only: fast (default), quality, or research (adds safety, trust, accessibility)."
    )]
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
    #[schemars(
        description = "Local OCR engine for ocr_pages: auto, vlm, or tesseract. Omit to use the configured command provider."
    )]
    pub ocr: Option<crate::ocr_vlm::OcrEngine>,
}
