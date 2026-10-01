use crate::error::Error;
use crate::mineru_diffusion::SdarConfig;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct HpdVisionConfig {
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    #[serde(default = "default_channels")]
    pub num_channels: usize,
    pub image_size: usize,
    pub patch_size: usize,
    #[serde(default = "default_eps")]
    pub layer_norm_eps: f64,
    #[serde(default = "default_act")]
    pub hidden_act: String,
    #[serde(default)]
    pub qkv_bias: bool,
    #[serde(default = "default_norm")]
    pub norm_type: String,
}

fn default_channels() -> usize {
    3
}

fn default_eps() -> f64 {
    1e-6
}

fn default_act() -> String {
    "gelu".to_string()
}

fn default_norm() -> String {
    "layer_norm".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct HpdParsingConfig {
    pub vision_config: HpdVisionConfig,
    pub llm_config: SdarConfig,
    pub downsample_ratio: f64,
    pub force_image_size: usize,
    #[serde(default = "default_min_patches")]
    pub min_dynamic_patch: usize,
    #[serde(default = "default_max_patches")]
    pub max_dynamic_patch: usize,
    #[serde(default = "crate::utils::default_true")]
    pub use_thumbnail: bool,
    pub fork_token_id: u32,
    pub child_token_id: u32,
    pub eos_token_id: u32,
    pub pad_token_id: u32,
}

fn default_min_patches() -> usize {
    1
}

fn default_max_patches() -> usize {
    24
}

impl HpdParsingConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let cfg: Self = crate::utils::load_json_config(path, "HPD-Parsing", "config.json")?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn image_tokens_per_tile(&self) -> Result<usize, Error> {
        let grid = self.force_image_size / self.vision_config.patch_size;
        let downsample = (1.0 / self.downsample_ratio).round() as usize;
        if downsample == 0 || !grid.is_multiple_of(downsample) {
            return Err(Error::Config {
                message: format!(
                    "HPD-Parsing patch grid {grid} is incompatible with downsample_ratio {}",
                    self.downsample_ratio
                ),
            });
        }
        Ok((grid / downsample).pow(2))
    }

    pub fn validate(&self) -> Result<(), Error> {
        let vision = &self.vision_config;
        if vision.hidden_size == 0
            || vision.intermediate_size == 0
            || vision.num_hidden_layers == 0
            || vision.num_attention_heads == 0
            || vision.patch_size == 0
            || self.force_image_size == 0
            || self.min_dynamic_patch == 0
            || self.max_dynamic_patch < self.min_dynamic_patch
        {
            return Err(Error::Config {
                message: "HPD-Parsing dimensions and patch limits must be non-zero and ordered"
                    .to_string(),
            });
        }
        if vision.num_channels != 3
            || self.force_image_size != vision.image_size
            || !vision
                .hidden_size
                .is_multiple_of(vision.num_attention_heads)
            || !self.force_image_size.is_multiple_of(vision.patch_size)
        {
            return Err(Error::Config {
                message:
                    "HPD-Parsing requires RGB square InternViT tiles with a valid head/patch split"
                        .to_string(),
            });
        }
        if vision.hidden_act != "gelu" || vision.norm_type != "layer_norm" {
            return Err(Error::Config {
                message: format!(
                    "HPD-Parsing unsupported InternViT activation/norm: {}/{}",
                    vision.hidden_act, vision.norm_type
                ),
            });
        }
        if !(0.0 < self.downsample_ratio && self.downsample_ratio <= 1.0) {
            return Err(Error::Config {
                message: "HPD-Parsing downsample_ratio must be in (0, 1]".to_string(),
            });
        }
        self.llm_config.head_dim()?;
        self.image_tokens_per_tile()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_config_shape() {
        let raw = r#"{
          "downsample_ratio":0.5,"force_image_size":448,
          "min_dynamic_patch":1,"max_dynamic_patch":24,"use_thumbnail":true,
          "fork_token_id":151679,"child_token_id":151680,"eos_token_id":151645,"pad_token_id":151643,
          "vision_config":{"hidden_size":1024,"intermediate_size":4096,"num_hidden_layers":24,
            "num_attention_heads":16,"num_channels":3,"image_size":448,"patch_size":14,
            "layer_norm_eps":0.000001,"hidden_act":"gelu","qkv_bias":true,"norm_type":"layer_norm"},
          "llm_config":{"vocab_size":151681,"hidden_size":1024,"intermediate_size":3072,
            "num_hidden_layers":28,"num_attention_heads":16,"num_key_value_heads":8,"head_dim":128,
            "max_position_embeddings":40960,"eos_token_id":151645,"pad_token_id":151643}
        }"#;
        let cfg: HpdParsingConfig = serde_json::from_str(raw).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.image_tokens_per_tile().unwrap(), 256);
    }
}
