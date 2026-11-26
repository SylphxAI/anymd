use anymd_formats::video::timeline::{FrameMetadata, Timeline};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterRegion {
    pub text: String,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    /// E.g. OCR region/word; never relabel layout regions as glyph geometry.
    pub geometry_level: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterObservation {
    pub provider: String,
    pub model_revision: Option<String>,
    pub coordinate_space: String,
    pub regions: Vec<RasterRegion>,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampledOcr {
    pub scene_id: String,
    pub frame: FrameMetadata,
    /// Supports this decoded frame only, not the entire scene interval.
    pub observation: RasterObservation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameDescription {
    pub text: String,
    pub provider: String,
    pub model_revision: Option<String>,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampledDescription {
    pub scene_id: String,
    pub frame: FrameMetadata,
    pub description: FrameDescription,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEvidence {
    pub timeline: Timeline,
    pub ocr_observations: Vec<SampledOcr>,
    pub descriptions: Vec<SampledDescription>,
}
