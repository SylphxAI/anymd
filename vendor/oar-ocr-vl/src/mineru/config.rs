use crate::error::Error;
use serde::Deserialize;
use std::path::Path;

fn default_vision_hidden_act() -> String {
    "quick_gelu".to_string()
}

fn default_text_hidden_act() -> String {
    "silu".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MinerURopeScaling {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub mrope_section: Vec<usize>,
}

/// Subset of the nested `text_config` block emitted by newer transformers
/// (>= 4.52) checkpoints such as `MinerU2.5-Pro-2605`. These checkpoints share
/// the Qwen2-VL backbone and the exact same weight layout as `MinerU2.5-2509`,
/// but relocate a handful of text-tower fields out of the config root and into
/// `text_config`. We only need `tie_word_embeddings` from it: the Pro config
/// omits the field at the root (so it would default to `false`), yet the
/// checkpoint ties the LM head to the input embeddings and ships no
/// `lm_head.weight` tensor. Resolving the effective flag from either location
/// keeps both the 2509 and Pro layouts loadable through the same path.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MinerUTextConfig {
    #[serde(default)]
    pub tie_word_embeddings: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinerUVisionConfig {
    pub depth: usize,
    pub embed_dim: usize,
    pub hidden_size: usize,
    #[serde(default = "default_vision_hidden_act")]
    pub hidden_act: String,
    pub mlp_ratio: f64,
    pub num_heads: usize,
    // Qwen2-VL checkpoints spell the input-channel count as `in_chans` (the
    // 2509 layout) or `in_channels` (Pro-2605); some newer configs emit BOTH
    // keys with identical values. A single field with two serde aliases trips
    // serde's duplicate-field detection when both keys are present, so capture
    // each spelling separately and resolve through `in_channels()`.
    #[serde(default, rename = "in_chans")]
    in_chans: Option<usize>,
    #[serde(default, rename = "in_channels")]
    in_channels_alias: Option<usize>,
    pub patch_size: usize,
    pub spatial_merge_size: usize,
    #[serde(default)]
    pub spatial_patch_size: Option<usize>,
    pub temporal_patch_size: usize,
}

impl MinerUVisionConfig {
    pub fn mlp_hidden_dim(&self) -> usize {
        (self.embed_dim as f64 * self.mlp_ratio).round() as usize
    }

    /// Input channel count, resolving the `in_chans`/`in_channels` spellings.
    /// Defaults to 3 (RGB) when neither key is present.
    pub fn in_channels(&self) -> usize {
        self.in_chans.or(self.in_channels_alias).unwrap_or(3)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinerUConfig {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    #[serde(default)]
    pub attention_dropout: f64,
    pub rms_norm_eps: f64,
    pub rope_theta: f64,
    pub max_position_embeddings: usize,
    #[serde(default)]
    pub sliding_window: Option<usize>,
    #[serde(default)]
    pub max_window_layers: usize,
    #[serde(default)]
    pub use_sliding_window: bool,
    #[serde(default = "default_text_hidden_act")]
    pub hidden_act: String,
    #[serde(default)]
    pub tie_word_embeddings: bool,
    #[serde(default)]
    pub bos_token_id: u32,
    pub eos_token_id: u32,
    #[serde(default)]
    pub pad_token_id: Option<u32>,
    pub vision_start_token_id: u32,
    pub vision_end_token_id: u32,
    pub vision_token_id: u32,
    pub image_token_id: u32,
    pub video_token_id: u32,
    #[serde(default)]
    pub rope_scaling: MinerURopeScaling,
    pub vision_config: MinerUVisionConfig,
    /// Nested text-tower config present in newer transformers checkpoints
    /// (e.g. `MinerU2.5-Pro-2605`). Absent on the original 2509 layout.
    #[serde(default)]
    pub text_config: MinerUTextConfig,
}

impl MinerUConfig {
    /// Effective `tie_word_embeddings` flag, honouring both the legacy root
    /// field (2509) and the newer nested `text_config` field (Pro-2605).
    pub fn tie_word_embeddings(&self) -> bool {
        self.tie_word_embeddings || self.text_config.tie_word_embeddings
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "MinerU2.5", "config.json")
    }

    pub fn head_dim(&self) -> Result<usize, Error> {
        if !self.hidden_size.is_multiple_of(self.num_attention_heads) {
            return Err(Error::Config {
                message: format!(
                    "MinerU2.5: hidden_size {} not divisible by num_attention_heads {}",
                    self.hidden_size, self.num_attention_heads
                ),
            });
        }
        Ok(self.hidden_size / self.num_attention_heads)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinerUImageProcessorConfig {
    #[serde(default)]
    pub min_pixels: Option<u32>,
    #[serde(default)]
    pub max_pixels: Option<u32>,
    #[serde(default)]
    pub size: Option<MinerUImageSize>,
    #[serde(default = "crate::utils::default_true")]
    pub do_resize: bool,
    #[serde(default = "crate::utils::default_true")]
    pub do_rescale: bool,
    #[serde(default = "crate::utils::default_true")]
    pub do_normalize: bool,
    #[serde(default = "crate::utils::default_true")]
    pub do_convert_rgb: bool,
    pub patch_size: usize,
    pub temporal_patch_size: usize,
    pub merge_size: usize,
    pub image_mean: Vec<f32>,
    pub image_std: Vec<f32>,
    #[serde(default)]
    pub resample: Option<u32>,
    #[serde(default = "crate::utils::default_rescale_factor")]
    pub rescale_factor: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinerUImageSize {
    pub shortest_edge: u32,
    pub longest_edge: u32,
}

impl MinerUImageProcessorConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "MinerU2.5", "preprocessor_config.json")
    }

    pub fn pixel_bounds(&self) -> Result<(u32, u32), Error> {
        if let Some(size) = &self.size {
            if size.shortest_edge == 0 || size.longest_edge == 0 {
                return Err(Error::Config {
                    message: "MinerU2.5 size.shortest_edge/longest_edge must be > 0".to_string(),
                });
            }
            return Ok((size.shortest_edge, size.longest_edge));
        }

        match (self.min_pixels, self.max_pixels) {
            (Some(min_pixels), Some(max_pixels)) => Ok((min_pixels, max_pixels)),
            _ => Err(Error::Config {
                message: "MinerU2.5 preprocessor_config missing size or min/max pixels".to_string(),
            }),
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.do_normalize {
            crate::utils::validate_image_mean_std("MinerU2.5", &self.image_mean, &self.image_std)?;
            if self.image_std.contains(&0.0) {
                return Err(Error::Config {
                    message: "MinerU2.5 image_std values must be non-zero".to_string(),
                });
            }
        }
        crate::utils::validate_patch_merge_temporal(
            "MinerU2.5",
            self.patch_size,
            self.merge_size,
            self.temporal_patch_size,
        )?;
        if self.do_resize {
            let (min_pixels, max_pixels) = self.pixel_bounds()?;
            if min_pixels == 0 || max_pixels == 0 {
                return Err(Error::Config {
                    message: "MinerU2.5 min/max pixels must be > 0".to_string(),
                });
            }
            if min_pixels > max_pixels {
                return Err(Error::Config {
                    message: format!(
                        "MinerU2.5 min_pixels ({min_pixels}) must be <= max_pixels ({max_pixels})"
                    ),
                });
            }
        }
        if self.do_rescale && self.rescale_factor <= 0.0 {
            return Err(Error::Config {
                message: "MinerU2.5 rescale_factor must be > 0".to_string(),
            });
        }
        Ok(())
    }
}
