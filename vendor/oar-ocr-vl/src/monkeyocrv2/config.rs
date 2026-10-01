use crate::error::Error;
use crate::mineru_diffusion::SdarConfig;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct MonkeyOcrV2VisionConfig {
    pub embed_dim: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    #[serde(default = "default_num_channels")]
    pub num_channels: usize,
    pub patch_size: usize,
    pub spatial_merge_size: usize,
    pub temporal_patch_size: usize,
    pub rms_norm_eps: f64,
    #[serde(default)]
    pub use_bias: bool,
    #[serde(default = "crate::utils::default_true")]
    pub post_norm: bool,
}

fn default_num_channels() -> usize {
    3
}

impl MonkeyOcrV2VisionConfig {
    pub fn head_dim(&self) -> Result<usize, Error> {
        if self.num_attention_heads == 0 || !self.embed_dim.is_multiple_of(self.num_attention_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 vision embed_dim {} must be divisible by num_attention_heads {}",
                    self.embed_dim, self.num_attention_heads
                ),
            });
        }
        Ok(self.embed_dim / self.num_attention_heads)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.embed_dim == 0
            || self.hidden_size == 0
            || self.intermediate_size == 0
            || self.num_hidden_layers == 0
            || self.patch_size == 0
            || self.spatial_merge_size == 0
            || self.temporal_patch_size == 0
        {
            return Err(Error::Config {
                message: "MonkeyOCRv2 vision dimensions must be non-zero".to_string(),
            });
        }
        if self.num_channels != 3 {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 currently supports RGB checkpoints, got num_channels={}",
                    self.num_channels
                ),
            });
        }
        if self.temporal_patch_size != 1 {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 image checkpoints require temporal_patch_size=1, got {}",
                    self.temporal_patch_size
                ),
            });
        }
        if self.use_bias {
            return Err(Error::Config {
                message: "MonkeyOCRv2 vision transformer use_bias=true is unsupported".to_string(),
            });
        }
        let head_dim = self.head_dim()?;
        if !head_dim.is_multiple_of(4) {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 vision head_dim {head_dim} must be divisible by 4 for 2D RoPE"
                ),
            });
        }
        if !self.rms_norm_eps.is_finite() || self.rms_norm_eps <= 0.0 {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 vision rms_norm_eps must be positive, got {}",
                    self.rms_norm_eps
                ),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MonkeyOcrV2Config {
    #[serde(flatten)]
    pub text_config: SdarConfig,
    pub vision_config: MonkeyOcrV2VisionConfig,
    pub image_token_id: u32,
    pub video_token_id: u32,
    pub model_type: String,
}

impl MonkeyOcrV2Config {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let config: Self = crate::utils::load_json_config(path, "MonkeyOCRv2", "config.json")?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.model_type != "monkeyocrv2" {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 expected model_type 'monkeyocrv2', got '{}'",
                    self.model_type
                ),
            });
        }
        if self.text_config.hidden_act != "silu" {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 supports Qwen3 hidden_act 'silu', got '{}'",
                    self.text_config.hidden_act
                ),
            });
        }
        if self.text_config.attention_bias {
            return Err(Error::Config {
                message: "MonkeyOCRv2 attention_bias=true is unsupported".to_string(),
            });
        }
        if self.text_config.vocab_size == 0
            || self.text_config.hidden_size == 0
            || self.text_config.num_hidden_layers == 0
            || self.text_config.num_attention_heads == 0
            || self.text_config.num_key_value_heads == 0
            || self.text_config.max_position_embeddings == 0
        {
            return Err(Error::Config {
                message: "MonkeyOCRv2 text dimensions must be non-zero".to_string(),
            });
        }
        if self.text_config.head_dim()? == 0 {
            return Err(Error::Config {
                message: "MonkeyOCRv2 text head_dim must be non-zero".to_string(),
            });
        }
        if !self
            .text_config
            .num_attention_heads
            .is_multiple_of(self.text_config.num_key_value_heads)
        {
            return Err(Error::Config {
                message: "MonkeyOCRv2 attention heads must be divisible by KV heads".to_string(),
            });
        }
        if self.text_config.hidden_size != self.vision_config.hidden_size {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 vision merger output {} != text hidden size {}",
                    self.vision_config.hidden_size, self.text_config.hidden_size
                ),
            });
        }
        self.vision_config.validate()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MonkeyOcrV2ImageProcessorConfig {
    pub min_pixels: u32,
    pub max_pixels: u32,
    pub patch_size: usize,
    pub temporal_patch_size: usize,
    pub merge_size: usize,
    pub image_mean: Vec<f32>,
    pub image_std: Vec<f32>,
    #[serde(default = "crate::utils::default_true")]
    pub do_resize: bool,
    #[serde(default = "crate::utils::default_true")]
    pub do_rescale: bool,
    #[serde(default = "crate::utils::default_true")]
    pub do_normalize: bool,
    #[serde(default = "crate::utils::default_rescale_factor")]
    pub rescale_factor: f32,
    #[serde(default)]
    pub resample: Option<u32>,
}

impl MonkeyOcrV2ImageProcessorConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let config: Self =
            crate::utils::load_json_config(path, "MonkeyOCRv2", "preprocessor_config.json")?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), Error> {
        crate::utils::validate_patch_merge_temporal(
            "MonkeyOCRv2",
            self.patch_size,
            self.merge_size,
            self.temporal_patch_size,
        )?;
        if self.min_pixels == 0 || self.max_pixels == 0 || self.min_pixels > self.max_pixels {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 invalid pixel bounds: {}..{}",
                    self.min_pixels, self.max_pixels
                ),
            });
        }
        if self.do_normalize {
            crate::utils::validate_image_mean_std(
                "MonkeyOCRv2",
                &self.image_mean,
                &self.image_std,
            )?;
            if self.image_std.contains(&0.0) {
                return Err(Error::Config {
                    message: "MonkeyOCRv2 image_std values must be non-zero".to_string(),
                });
            }
        }
        Ok(())
    }

    pub fn validate_vision(&self, vision: &MonkeyOcrV2VisionConfig) -> Result<(), Error> {
        if self.patch_size != vision.patch_size
            || self.temporal_patch_size != vision.temporal_patch_size
            || self.merge_size != vision.spatial_merge_size
        {
            return Err(Error::Config {
                message: format!(
                    "MonkeyOCRv2 processor/vision mismatch: patch {}/{}, temporal {}/{}, merge {}/{}",
                    self.patch_size,
                    vision.patch_size,
                    self.temporal_patch_size,
                    vision.temporal_patch_size,
                    self.merge_size,
                    vision.spatial_merge_size
                ),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shipped_s_config_shape() {
        let config: MonkeyOcrV2Config = serde_json::from_str(
            r#"{
              "model_type":"monkeyocrv2","vocab_size":151936,"hidden_size":1024,
              "intermediate_size":3072,"num_hidden_layers":28,"num_attention_heads":16,
              "num_key_value_heads":8,"head_dim":128,"max_position_embeddings":40960,
              "attention_bias":false,"tie_word_embeddings":true,"rope_theta":1000000,
              "rms_norm_eps":0.000001,"hidden_act":"silu","image_token_id":151655,
              "video_token_id":151656,
              "vision_config":{"embed_dim":384,"hidden_size":1024,
                "intermediate_size":1536,"num_hidden_layers":12,"num_attention_heads":6,
                "num_channels":3,"patch_size":14,"spatial_merge_size":2,
                "temporal_patch_size":1,"rms_norm_eps":0.00001,"use_bias":false,
                "post_norm":true}
            }"#,
        )
        .unwrap();
        config.validate().unwrap();
        assert_eq!(config.text_config.head_dim().unwrap(), 128);
        assert_eq!(config.vision_config.head_dim().unwrap(), 64);
    }

    #[test]
    fn parses_shipped_b_config_shape() {
        let config: MonkeyOcrV2Config = serde_json::from_str(
            r#"{
              "model_type":"monkeyocrv2","vocab_size":151936,"hidden_size":1024,
              "intermediate_size":3072,"num_hidden_layers":28,"num_attention_heads":16,
              "num_key_value_heads":8,"head_dim":128,"max_position_embeddings":40960,
              "attention_bias":false,"tie_word_embeddings":true,"rope_theta":1000000,
              "rms_norm_eps":0.000001,"hidden_act":"silu","image_token_id":151655,
              "video_token_id":151656,
              "vision_config":{"embed_dim":768,"hidden_size":1024,
                "intermediate_size":3072,"num_hidden_layers":12,"num_attention_heads":12,
                "num_channels":3,"patch_size":14,"spatial_merge_size":2,
                "temporal_patch_size":1,"rms_norm_eps":0.00001,"use_bias":false,
                "post_norm":true}
            }"#,
        )
        .unwrap();
        config.validate().unwrap();
        assert_eq!(config.text_config.head_dim().unwrap(), 128);
        assert_eq!(config.vision_config.embed_dim, 768);
        assert_eq!(config.vision_config.num_attention_heads, 12);
        assert_eq!(config.vision_config.head_dim().unwrap(), 64);
    }
}
