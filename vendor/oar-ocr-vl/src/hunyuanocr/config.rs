use crate::error::Error;
use serde::Deserialize;
use std::path::Path;

/// Checkpoint generation supported by the HunyuanOCR backend.
///
/// The upstream config does not expose a dedicated version field. V1.5 uses
/// the newer nested `text_config` schema, while the archived V1.0 checkpoint
/// keeps all text fields at the top level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunyuanOcrVersion {
    V1,
    V1_5,
}

impl std::fmt::Display for HunyuanOcrVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V1 => f.write_str("1.0"),
            Self::V1_5 => f.write_str("1.5"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HunyuanOcrRopeScaling {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub xdrope_section: Vec<usize>,
    #[serde(default)]
    pub factor: Option<f64>,
    #[serde(default)]
    pub alpha: Option<f64>,
    #[serde(default)]
    pub beta_fast: Option<usize>,
    #[serde(default)]
    pub beta_slow: Option<usize>,
    #[serde(default)]
    pub mscale: Option<f64>,
    #[serde(default)]
    pub mscale_all_dim: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HunyuanOcrVisionConfig {
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub num_channels: usize,
    pub patch_size: usize,
    pub spatial_merge_size: usize,
    pub rms_norm_eps: f64,
    pub hidden_act: String,
    pub add_patchemb_bias: bool,
    pub cat_extra_token: usize,
    pub max_vit_seq_len: usize,
    pub max_image_size: usize,
    pub img_max_token_num: usize,
    pub interpolate_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HunyuanOcrConfig {
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
    pub vocab_size: usize,
    pub rms_norm_eps: f64,
    pub rope_theta: f64,

    pub bos_token_id: u32,
    pub eos_token_id: u32,
    pub eod_token_id: u32,
    pub pad_id: u32,

    pub image_start_token_id: u32,
    pub image_end_token_id: u32,
    pub image_token_id: u32,
    pub image_newline_token_id: u32,

    pub use_qk_norm: bool,
    pub rope_scaling: HunyuanOcrRopeScaling,
    pub vision_config: HunyuanOcrVisionConfig,

    /// Present in HunyuanOCR 1.5's Transformers config. The text backbone
    /// parameters remain duplicated at the top level for checkpoint
    /// compatibility, so only presence is needed for version detection.
    #[serde(default)]
    pub text_config: Option<serde_json::Value>,
}

impl HunyuanOcrConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "HunyuanOCR", "config.json")
    }

    pub fn version(&self) -> HunyuanOcrVersion {
        if self
            .text_config
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .is_some()
        {
            HunyuanOcrVersion::V1_5
        } else {
            HunyuanOcrVersion::V1
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HunyuanOcrImageProcessorConfig {
    pub min_pixels: u32,
    pub max_pixels: u32,
    pub patch_size: usize,
    #[serde(default)]
    pub resample: Option<u32>,
    pub temporal_patch_size: usize,
    pub merge_size: usize,
    pub image_mean: Vec<f32>,
    pub image_std: Vec<f32>,
}

impl HunyuanOcrImageProcessorConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "HunyuanOCR", "preprocessor_config.json")
    }

    pub fn validate(&self) -> Result<(), Error> {
        crate::utils::validate_image_mean_std("HunyuanOCR", &self.image_mean, &self.image_std)?;
        crate::utils::validate_patch_merge_temporal(
            "HunyuanOCR",
            self.patch_size,
            self.merge_size,
            self.temporal_patch_size,
        )
    }
}
