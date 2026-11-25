use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OcrWord {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_order: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_type: Option<String>,
    /// Layout classification confidence, never recognition confidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OcrPage {
    pub page: u32,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<OcrWord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub provider: String,
    pub source_render_evidence_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_render_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_render_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_render_height: Option<u32>,
    pub provenance: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceOcrOutcome {
    pub source_index: usize,
    pub pages: Vec<OcrPage>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}
