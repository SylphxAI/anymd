use crate::error::Error;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct PaddleOcrVlImageProcessorConfig {
    pub do_resize: bool,
    pub do_rescale: bool,
    pub do_normalize: bool,
    pub do_convert_rgb: bool,
    pub rescale_factor: f32,
    pub image_mean: Vec<f32>,
    pub image_std: Vec<f32>,
    pub min_pixels: u32,
    pub max_pixels: u32,
    #[serde(default)]
    pub resample: Option<u32>,
    pub patch_size: usize,
    pub temporal_patch_size: usize,
    pub merge_size: usize,
}

impl PaddleOcrVlImageProcessorConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "PaddleOCR-VL", "preprocessor_config.json")
    }

    pub fn validate(&self) -> Result<(), Error> {
        crate::utils::validate_image_mean_std("PaddleOCR-VL", &self.image_mean, &self.image_std)?;
        crate::utils::validate_patch_merge_temporal(
            "PaddleOCR-VL",
            self.patch_size,
            self.merge_size,
            self.temporal_patch_size,
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaddleOcrVlRopeScaling {
    pub mrope_section: Vec<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaddleOcrVlVisionConfig {
    pub image_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub num_channels: usize,
    pub patch_size: usize,
    pub spatial_merge_size: usize,
    pub temporal_patch_size: usize,
    pub tokens_per_second: usize,
    pub layer_norm_eps: f64,
    pub hidden_act: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaddleOcrVlConfig {
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
    pub vocab_size: usize,
    pub rms_norm_eps: f64,
    pub rope_theta: f64,
    pub image_token_id: u32,
    pub video_token_id: u32,
    pub vision_start_token_id: u32,
    pub vision_end_token_id: u32,
    pub hidden_act: String,
    pub use_bias: bool,
    pub rope_scaling: PaddleOcrVlRopeScaling,
    pub vision_config: PaddleOcrVlVisionConfig,
}

impl PaddleOcrVlConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "PaddleOCR-VL", "config.json")
    }
}
