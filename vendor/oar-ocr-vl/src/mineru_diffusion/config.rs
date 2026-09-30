//! Configuration types for MinerU-Diffusion-V1.
//!
//! The checkpoint nests two sub-configs: a Qwen2-VL vision tower
//! (`vision_config`, reusing [`MinerUVisionConfig`]) and an `SDAR` block-
//! diffusion text decoder (`text_config`). The top-level config carries the
//! multimodal token ids, the mask token used by the diffusion denoiser, and
//! the vision projector type.

use crate::error::Error;
use crate::mineru::MinerUVisionConfig;
use serde::Deserialize;
use std::path::Path;

fn default_text_hidden_act() -> String {
    "silu".to_string()
}

fn default_rms_norm_eps() -> f64 {
    1e-6
}

fn default_rope_theta() -> f64 {
    1_000_000.0
}

/// `SDAR` text-decoder config (block-diffusion Qwen2-style backbone with
/// QK-norm and non-causal block attention).
#[derive(Debug, Clone, Deserialize)]
pub struct SdarConfig {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    /// Explicit per-head dim; SDAR sets this to 128 independently of
    /// `hidden_size / num_attention_heads`. Falls back to that ratio if absent.
    #[serde(default)]
    pub head_dim: Option<usize>,
    #[serde(default = "default_text_hidden_act")]
    pub hidden_act: String,
    #[serde(default = "default_rms_norm_eps")]
    pub rms_norm_eps: f64,
    #[serde(default = "default_rope_theta")]
    pub rope_theta: f64,
    pub max_position_embeddings: usize,
    #[serde(default)]
    pub attention_bias: bool,
    #[serde(default)]
    pub tie_word_embeddings: bool,
    #[serde(default)]
    pub eos_token_id: Option<u32>,
    #[serde(default)]
    pub bos_token_id: Option<u32>,
    #[serde(default)]
    pub pad_token_id: Option<u32>,
}

impl SdarConfig {
    /// Resolve the per-head dimension, honouring the explicit `head_dim`.
    pub fn head_dim(&self) -> Result<usize, Error> {
        if let Some(hd) = self.head_dim {
            return Ok(hd);
        }
        if !self.hidden_size.is_multiple_of(self.num_attention_heads) {
            return Err(Error::Config {
                message: format!(
                    "MinerU-Diffusion: hidden_size {} not divisible by num_attention_heads {}",
                    self.hidden_size, self.num_attention_heads
                ),
            });
        }
        Ok(self.hidden_size / self.num_attention_heads)
    }
}

/// Top-level MinerU-Diffusion config (`model_type = "mineru_diffusion"`).
#[derive(Debug, Clone, Deserialize)]
pub struct MinerUDiffusionConfig {
    /// SDAR text decoder. The HF config also exposes this as
    /// `language_model_config`, but the on-disk JSON key is `text_config`.
    #[serde(alias = "language_model_config")]
    pub text_config: SdarConfig,
    /// Qwen2-VL vision tower. On-disk JSON key is `vision_config`
    /// (`vision_model_config` is a runtime alias).
    #[serde(alias = "vision_model_config")]
    pub vision_config: MinerUVisionConfig,
    pub image_token_id: u32,
    #[serde(default)]
    pub video_token_id: u32,
    pub vision_start_token_id: u32,
    pub vision_end_token_id: u32,
    /// Token id used to seed every not-yet-decoded position in the diffusion
    /// generation buffer (`<|MASK|>`, 151669).
    pub mask_token_id: u32,
    /// Projector flavour. Only `patch_merger<N>x` / `pm<N>x` are supported.
    pub vision_projector_type: String,
}

impl MinerUDiffusionConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        crate::utils::load_json_config(path, "MinerU-Diffusion", "config.json")
    }

    /// Spatial merge factor encoded in `vision_projector_type`
    /// (`patch_merger2x` / `pm2x` -> 2). Falls back to the vision tower's
    /// `spatial_merge_size` when the type carries no explicit factor.
    pub fn projector_merge_size(&self) -> Result<usize, Error> {
        parse_merge_size(
            &self.vision_projector_type,
            self.vision_config.spatial_merge_size,
        )
    }
}

/// Parse the spatial merge factor out of a `vision_projector_type` string
/// (`patch_merger2x` / `pm2x` -> 2). Returns `fallback` when the type carries
/// no explicit factor; errors on a malformed factor.
fn parse_merge_size(projector_type: &str, fallback: usize) -> Result<usize, Error> {
    let digits: String = projector_type
        .trim()
        .trim_start_matches("patch_merger")
        .trim_start_matches("pm")
        .trim_end_matches('x')
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return Ok(fallback);
    }
    digits.parse::<usize>().map_err(|_| Error::Config {
        message: format!("MinerU-Diffusion: unsupported vision_projector_type '{projector_type}'"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sdar_json(extra: &str) -> String {
        format!(
            r#"{{"vocab_size":151936,"hidden_size":2048,"intermediate_size":6144,
            "num_hidden_layers":28,"num_attention_heads":16,"num_key_value_heads":8,
            "max_position_embeddings":32768{extra}}}"#
        )
    }

    #[test]
    fn head_dim_prefers_explicit() {
        // hidden_size / heads = 2048 / 16 = 128, but an explicit head_dim wins
        // even when it diverges from that ratio (SDAR sets 128 independently).
        let cfg: SdarConfig = serde_json::from_str(&sdar_json(r#","head_dim":96"#)).unwrap();
        assert_eq!(cfg.head_dim().unwrap(), 96);
    }

    #[test]
    fn head_dim_falls_back_to_ratio() {
        let cfg: SdarConfig = serde_json::from_str(&sdar_json("")).unwrap();
        assert_eq!(cfg.head_dim().unwrap(), 128);
    }

    #[test]
    fn head_dim_rejects_indivisible_ratio() {
        // No explicit head_dim and hidden_size (2048) not divisible by 17 heads.
        let cfg: SdarConfig = serde_json::from_str(
            r#"{"vocab_size":151936,"hidden_size":2048,"intermediate_size":6144,
            "num_hidden_layers":28,"num_attention_heads":17,"num_key_value_heads":8,
            "max_position_embeddings":32768}"#,
        )
        .unwrap();
        assert!(cfg.head_dim().is_err());
    }

    #[test]
    fn merge_size_parses_factor() {
        assert_eq!(parse_merge_size("patch_merger2x", 99).unwrap(), 2);
        assert_eq!(parse_merge_size("pm4x", 99).unwrap(), 4);
        assert_eq!(parse_merge_size("  patch_merger2x  ", 99).unwrap(), 2);
    }

    #[test]
    fn merge_size_falls_back_when_no_factor() {
        // No digits in the type -> use the vision tower's spatial_merge_size.
        assert_eq!(parse_merge_size("patch_merger", 2).unwrap(), 2);
        assert_eq!(parse_merge_size("pm", 3).unwrap(), 3);
    }
}
