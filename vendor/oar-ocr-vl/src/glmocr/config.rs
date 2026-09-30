use crate::error::Error;
use candle_nn::Activation;
use serde::Deserialize;
use std::path::Path;

fn default_partial_rotary_factor() -> f64 {
    1.0
}

fn default_rope_theta() -> f64 {
    10000.0
}

fn default_in_channels() -> usize {
    3
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmOcrRopeParameters {
    pub rope_type: String,
    pub mrope_section: Vec<usize>,
    #[serde(default = "default_partial_rotary_factor")]
    pub partial_rotary_factor: f64,
    #[serde(default = "default_rope_theta")]
    pub rope_theta: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EosTokenId {
    Single(u32),
    Multiple(Vec<u32>),
}

impl EosTokenId {
    pub fn to_vec(&self) -> Vec<u32> {
        match self {
            Self::Single(v) => vec![*v],
            Self::Multiple(v) => v.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmOcrTextConfig {
    pub model_type: String,
    pub pad_token_id: u32,
    pub vocab_size: usize,
    pub eos_token_id: EosTokenId,
    pub attention_bias: bool,
    #[serde(default)]
    pub attention_dropout: f32,
    pub head_dim: usize,
    pub hidden_act: Activation,
    pub hidden_size: usize,
    #[serde(default)]
    pub initializer_range: f64,
    pub intermediate_size: usize,
    pub max_position_embeddings: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    #[serde(default)]
    pub num_nextn_predict_layers: usize,
    pub num_key_value_heads: usize,
    pub rms_norm_eps: f64,
    pub rope_parameters: GlmOcrRopeParameters,
    #[serde(default)]
    pub tie_word_embeddings: bool,
    #[serde(default)]
    pub use_cache: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmOcrVisionConfig {
    pub model_type: String,
    pub hidden_size: usize,
    pub depth: usize,
    pub num_heads: usize,
    pub attention_bias: bool,
    #[serde(default)]
    pub attention_dropout: f32,
    #[serde(default = "default_in_channels")]
    pub in_channels: usize,
    pub intermediate_size: usize,
    pub hidden_act: Activation,
    pub image_size: usize,
    pub patch_size: usize,
    pub out_hidden_size: usize,
    pub rms_norm_eps: f64,
    pub spatial_merge_size: usize,
    pub temporal_patch_size: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmOcrConfig {
    pub architectures: Vec<String>,
    pub model_type: String,
    pub text_config: GlmOcrTextConfig,
    pub vision_config: GlmOcrVisionConfig,
    pub image_start_token_id: u32,
    pub image_end_token_id: u32,
    pub video_start_token_id: u32,
    pub video_end_token_id: u32,
    pub image_token_id: u32,
    pub video_token_id: u32,
    #[serde(default)]
    pub tie_word_embeddings: bool,
    #[serde(default)]
    pub transformers_version: Option<String>,
}

impl GlmOcrConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "GLM-OCR", "config.json")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmOcrImageProcessorSize {
    pub shortest_edge: u32,
    pub longest_edge: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlmOcrImageProcessorConfig {
    pub size: GlmOcrImageProcessorSize,
    pub do_rescale: bool,
    #[serde(default = "crate::utils::default_true")]
    pub do_resize: bool,
    #[serde(default = "crate::utils::default_true")]
    pub do_normalize: bool,
    #[serde(default = "crate::utils::default_rescale_factor")]
    pub rescale_factor: f32,
    #[serde(default = "crate::utils::default_true")]
    pub do_convert_rgb: bool,
    #[serde(default)]
    pub resample: Option<u32>,
    pub patch_size: usize,
    pub temporal_patch_size: usize,
    pub merge_size: usize,
    pub image_mean: Vec<f32>,
    pub image_std: Vec<f32>,
    #[serde(default)]
    pub image_processor_type: Option<String>,
    #[serde(default)]
    pub processor_class: Option<String>,
}

impl GlmOcrImageProcessorConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "GLM-OCR", "preprocessor_config.json")
    }

    pub fn validate(&self) -> Result<(), Error> {
        crate::utils::validate_image_mean_std("GLM-OCR", &self.image_mean, &self.image_std)?;
        if self.image_std.contains(&0.0) {
            return Err(Error::Config {
                message: "GLM-OCR image_std values must be non-zero (used as divisor)".to_string(),
            });
        }
        if self.patch_size == 0 {
            return Err(Error::Config {
                message: "GLM-OCR patch_size must be > 0".to_string(),
            });
        }
        if self.merge_size == 0 {
            return Err(Error::Config {
                message: "GLM-OCR merge_size must be > 0".to_string(),
            });
        }
        if self.temporal_patch_size == 0 {
            return Err(Error::Config {
                message: "GLM-OCR temporal_patch_size must be > 0".to_string(),
            });
        }
        if self.size.shortest_edge == 0 || self.size.longest_edge == 0 {
            return Err(Error::Config {
                message: "GLM-OCR size.shortest_edge/longest_edge must be > 0".to_string(),
            });
        }
        Ok(())
    }
}
