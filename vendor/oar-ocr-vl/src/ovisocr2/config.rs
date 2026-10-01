use crate::error::Error;
use candle_nn::Activation;
use serde::Deserialize;
use std::path::Path;

/// Minimum image area used by the official OvisOCR2 runtime.
pub const OVIS_OCR2_MIN_PIXELS: u32 = 448 * 448;
/// Maximum image area used by the official OvisOCR2 runtime.
pub const OVIS_OCR2_MAX_PIXELS: u32 = 2880 * 2880;

fn default_true() -> bool {
    true
}

fn default_rescale_factor() -> f32 {
    1.0 / 255.0
}

fn default_partial_rotary_factor() -> f64 {
    0.25
}

fn default_rope_theta() -> f64 {
    10_000_000.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct OvisOcr2RopeParameters {
    pub rope_type: String,
    pub mrope_section: Vec<usize>,
    #[serde(default)]
    pub mrope_interleaved: bool,
    #[serde(default = "default_rope_theta")]
    pub rope_theta: f64,
    #[serde(default = "default_partial_rotary_factor")]
    pub partial_rotary_factor: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OvisOcr2TextConfig {
    pub model_type: String,
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
    pub hidden_act: Activation,
    pub max_position_embeddings: usize,
    pub rms_norm_eps: f64,
    pub rope_parameters: OvisOcr2RopeParameters,
    pub layer_types: Vec<String>,
    pub linear_conv_kernel_dim: usize,
    pub linear_key_head_dim: usize,
    pub linear_value_head_dim: usize,
    pub linear_num_key_heads: usize,
    pub linear_num_value_heads: usize,
    pub eos_token_id: u32,
    #[serde(default)]
    pub attention_bias: bool,
    #[serde(default)]
    pub attention_dropout: f32,
    #[serde(default)]
    pub attn_output_gate: bool,
    #[serde(default)]
    pub initializer_range: f64,
    #[serde(default)]
    pub full_attention_interval: usize,
    #[serde(default)]
    pub mlp_only_layers: Vec<usize>,
    #[serde(default)]
    pub mtp_num_hidden_layers: usize,
    #[serde(default)]
    pub mtp_use_dedicated_embeddings: bool,
    #[serde(default)]
    pub tie_word_embeddings: bool,
    #[serde(default)]
    pub use_cache: bool,
    #[serde(default)]
    pub dtype: Option<String>,
    #[serde(default)]
    pub mamba_ssm_dtype: Option<String>,
}

impl OvisOcr2TextConfig {
    pub fn validate(&self) -> Result<(), Error> {
        if self.hidden_size == 0
            || self.intermediate_size == 0
            || self.vocab_size == 0
            || self.num_hidden_layers == 0
            || self.num_attention_heads == 0
            || self.num_key_value_heads == 0
            || self.head_dim == 0
            || self.max_position_embeddings == 0
        {
            return Err(Error::Config {
                message: "OvisOCR2 text dimensions must be non-zero".to_string(),
            });
        }
        if self.model_type != "qwen3_5_text" {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 expected text model_type 'qwen3_5_text', got '{}'",
                    self.model_type
                ),
            });
        }
        if self.hidden_act != Activation::Silu {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 text decoder supports hidden_act 'silu', got {:?}",
                    self.hidden_act
                ),
            });
        }
        if self.attention_bias {
            return Err(Error::Config {
                message: "OvisOCR2 attention_bias=true is not supported".to_string(),
            });
        }
        if !self.attn_output_gate {
            return Err(Error::Config {
                message: "OvisOCR2 requires attn_output_gate=true".to_string(),
            });
        }
        if !self.rms_norm_eps.is_finite() || self.rms_norm_eps <= 0.0 {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 rms_norm_eps must be finite and positive, got {}",
                    self.rms_norm_eps
                ),
            });
        }
        if self.eos_token_id as usize >= self.vocab_size {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 eos_token_id {} is outside vocab_size {}",
                    self.eos_token_id, self.vocab_size
                ),
            });
        }
        if !self
            .num_attention_heads
            .is_multiple_of(self.num_key_value_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 num_attention_heads ({}) must be divisible by num_key_value_heads ({})",
                    self.num_attention_heads, self.num_key_value_heads
                ),
            });
        }
        if self.layer_types.len() != self.num_hidden_layers {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 layer_types length ({}) must equal num_hidden_layers ({})",
                    self.layer_types.len(),
                    self.num_hidden_layers
                ),
            });
        }
        if self
            .layer_types
            .iter()
            .any(|kind| kind != "linear_attention" && kind != "full_attention")
        {
            return Err(Error::Config {
                message: "OvisOCR2 layer_types contains an unsupported layer type".to_string(),
            });
        }
        if self.linear_conv_kernel_dim == 0
            || self.linear_key_head_dim == 0
            || self.linear_value_head_dim == 0
            || self.linear_num_key_heads == 0
            || self.linear_num_value_heads == 0
        {
            return Err(Error::Config {
                message: "OvisOCR2 linear-attention dimensions must be non-zero".to_string(),
            });
        }
        if self.linear_key_head_dim != self.linear_value_head_dim {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 requires equal linear key/value head dims, got {}/{}",
                    self.linear_key_head_dim, self.linear_value_head_dim
                ),
            });
        }
        if !self
            .linear_num_value_heads
            .is_multiple_of(self.linear_num_key_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 linear_num_value_heads ({}) must be divisible by linear_num_key_heads ({})",
                    self.linear_num_value_heads, self.linear_num_key_heads
                ),
            });
        }
        if self.rope_parameters.rope_type != "default" {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 unsupported rope_type '{}'",
                    self.rope_parameters.rope_type
                ),
            });
        }
        if !self.rope_parameters.mrope_interleaved {
            return Err(Error::Config {
                message: "OvisOCR2 requires interleaved MRoPE".to_string(),
            });
        }
        if self.rope_parameters.mrope_section.len() != 3
            || self.rope_parameters.mrope_section.contains(&0)
        {
            return Err(Error::Config {
                message: "OvisOCR2 mrope_section must contain three non-zero entries".to_string(),
            });
        }
        if !self.rope_parameters.rope_theta.is_finite() || self.rope_parameters.rope_theta <= 0.0 {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 rope_theta must be finite and positive, got {}",
                    self.rope_parameters.rope_theta
                ),
            });
        }
        let partial = self.rope_parameters.partial_rotary_factor;
        if !partial.is_finite() || !(0.0..=1.0).contains(&partial) || partial == 0.0 {
            return Err(Error::Config {
                message: format!("OvisOCR2 partial_rotary_factor must be in (0, 1], got {partial}"),
            });
        }
        let rotary_dim = (self.head_dim as f64 * partial) as usize;
        let section_sum = self
            .rope_parameters
            .mrope_section
            .iter()
            .try_fold(0usize, |sum, &value| sum.checked_add(value))
            .ok_or_else(|| Error::Config {
                message: "OvisOCR2 mrope_section sum overflow".to_string(),
            })?;
        if rotary_dim == 0 || !rotary_dim.is_multiple_of(2) || section_sum != rotary_dim / 2 {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 mrope_section {:?} must sum to rotary_dim/2 ({})",
                    self.rope_parameters.mrope_section,
                    rotary_dim / 2
                ),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OvisOcr2VisionConfig {
    pub model_type: String,
    pub depth: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_heads: usize,
    pub in_channels: usize,
    pub patch_size: usize,
    pub spatial_merge_size: usize,
    pub temporal_patch_size: usize,
    pub out_hidden_size: usize,
    pub num_position_embeddings: usize,
    pub hidden_act: Activation,
    #[serde(default)]
    pub initializer_range: f64,
    #[serde(default)]
    pub deepstack_visual_indexes: Vec<usize>,
}

impl OvisOcr2VisionConfig {
    pub fn head_dim(&self) -> Result<usize, Error> {
        if self.num_heads == 0 || !self.hidden_size.is_multiple_of(self.num_heads) {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 vision hidden_size {} must be divisible by num_heads {}",
                    self.hidden_size, self.num_heads
                ),
            });
        }
        Ok(self.hidden_size / self.num_heads)
    }

    pub fn position_grid_size(&self) -> Result<usize, Error> {
        let side = (self.num_position_embeddings as f64).sqrt() as usize;
        if side == 0 || side * side != self.num_position_embeddings {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 num_position_embeddings must be a non-zero square, got {}",
                    self.num_position_embeddings
                ),
            });
        }
        Ok(side)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.model_type != "qwen3_5" {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 expected vision model_type 'qwen3_5', got '{}'",
                    self.model_type
                ),
            });
        }
        if self.hidden_size == 0
            || self.intermediate_size == 0
            || self.in_channels == 0
            || self.patch_size == 0
            || self.spatial_merge_size == 0
            || self.temporal_patch_size == 0
            || self.out_hidden_size == 0
        {
            return Err(Error::Config {
                message: "OvisOCR2 vision dimensions must be non-zero".to_string(),
            });
        }
        let head_dim = self.head_dim()?;
        if !head_dim.is_multiple_of(4) {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 vision head_dim must be divisible by 4 for 2D RoPE, got {head_dim}"
                ),
            });
        }
        self.position_grid_size()?;
        if !self.deepstack_visual_indexes.is_empty() {
            return Err(Error::Config {
                message: "OvisOCR2 deepstack vision features are not supported".to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OvisOcr2Config {
    pub architectures: Vec<String>,
    pub model_type: String,
    pub text_config: OvisOcr2TextConfig,
    pub vision_config: OvisOcr2VisionConfig,
    pub image_token_id: u32,
    pub video_token_id: u32,
    pub vision_start_token_id: u32,
    pub vision_end_token_id: u32,
    #[serde(default)]
    pub tie_word_embeddings: bool,
    #[serde(default)]
    pub transformers_version: Option<String>,
}

impl OvisOcr2Config {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let cfg: Self = crate::utils::load_json_config(path, "OvisOCR2", "config.json")?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn tie_word_embeddings(&self) -> bool {
        self.tie_word_embeddings || self.text_config.tie_word_embeddings
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.model_type != "qwen3_5" {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 expected model_type 'qwen3_5', got '{}'",
                    self.model_type
                ),
            });
        }
        self.text_config.validate()?;
        self.vision_config.validate()?;
        if !self.tie_word_embeddings() {
            return Err(Error::Config {
                message: "OvisOCR2 requires tied token embeddings".to_string(),
            });
        }
        for (name, token_id) in [
            ("image_token_id", self.image_token_id),
            ("video_token_id", self.video_token_id),
            ("vision_start_token_id", self.vision_start_token_id),
            ("vision_end_token_id", self.vision_end_token_id),
        ] {
            if token_id as usize >= self.text_config.vocab_size {
                return Err(Error::Config {
                    message: format!(
                        "OvisOCR2 {name} {token_id} is outside vocab_size {}",
                        self.text_config.vocab_size
                    ),
                });
            }
        }
        if self.vision_config.out_hidden_size != self.text_config.hidden_size {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 vision out_hidden_size ({}) must equal text hidden_size ({})",
                    self.vision_config.out_hidden_size, self.text_config.hidden_size
                ),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OvisOcr2ImageProcessorSize {
    pub shortest_edge: u32,
    pub longest_edge: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OvisOcr2ImageProcessorConfig {
    pub size: OvisOcr2ImageProcessorSize,
    pub patch_size: usize,
    pub temporal_patch_size: usize,
    pub merge_size: usize,
    pub image_mean: Vec<f32>,
    pub image_std: Vec<f32>,
    #[serde(default = "default_true")]
    pub do_resize: bool,
    #[serde(default = "default_true")]
    pub do_rescale: bool,
    #[serde(default = "default_true")]
    pub do_normalize: bool,
    #[serde(default = "default_true")]
    pub do_convert_rgb: bool,
    #[serde(default = "default_rescale_factor")]
    pub rescale_factor: f32,
    #[serde(default)]
    pub resample: Option<u32>,
    #[serde(default)]
    pub processor_class: Option<String>,
    #[serde(default)]
    pub image_processor_type: Option<String>,
}

impl OvisOcr2ImageProcessorConfig {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let cfg: Self =
            crate::utils::load_json_config(path, "OvisOCR2", "preprocessor_config.json")?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Pixel bounds used for OvisOCR2 inference.
    ///
    /// The checkpoint's generic Qwen processor metadata advertises a wider
    /// `256²..4096²` range. OvisOCR2's official inference wrapper overrides it
    /// with `448²..2880²`, which is the range used by this native backend.
    pub const fn runtime_pixel_bounds(&self) -> (u32, u32) {
        (OVIS_OCR2_MIN_PIXELS, OVIS_OCR2_MAX_PIXELS)
    }

    pub fn validate(&self) -> Result<(), Error> {
        crate::utils::validate_image_mean_std("OvisOCR2", &self.image_mean, &self.image_std)?;
        if self
            .image_mean
            .iter()
            .chain(&self.image_std)
            .any(|value| !value.is_finite())
        {
            return Err(Error::Config {
                message: "OvisOCR2 image_mean/std values must be finite".to_string(),
            });
        }
        if self.do_normalize && self.image_std.iter().any(|&value| value <= 0.0) {
            return Err(Error::Config {
                message: "OvisOCR2 image_std values must be positive".to_string(),
            });
        }
        if self.do_rescale && !self.rescale_factor.is_finite() {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 rescale_factor must be finite, got {}",
                    self.rescale_factor
                ),
            });
        }
        crate::utils::validate_patch_merge_temporal(
            "OvisOCR2",
            self.patch_size,
            self.merge_size,
            self.temporal_patch_size,
        )?;
        if self.size.shortest_edge == 0 || self.size.longest_edge == 0 {
            return Err(Error::Config {
                message: "OvisOCR2 processor size bounds must be non-zero".to_string(),
            });
        }
        if self.size.shortest_edge > self.size.longest_edge {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 shortest_edge {} exceeds longest_edge {}",
                    self.size.shortest_edge, self.size.longest_edge
                ),
            });
        }
        if let Some(resample) = self.resample
            && resample > 5
        {
            return Err(Error::Config {
                message: format!("OvisOCR2 unsupported PIL resample value {resample}"),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
    {
      "architectures": ["Qwen3_5ForConditionalGeneration"],
      "image_token_id": 248056,
      "model_type": "qwen3_5",
      "text_config": {
        "attention_bias": false,
        "attention_dropout": 0.0,
        "attn_output_gate": true,
        "dtype": "bfloat16",
        "eos_token_id": 248044,
        "full_attention_interval": 4,
        "head_dim": 256,
        "hidden_act": "silu",
        "hidden_size": 1024,
        "initializer_range": 0.02,
        "intermediate_size": 3584,
        "layer_types": ["linear_attention", "linear_attention", "linear_attention", "full_attention"],
        "linear_conv_kernel_dim": 4,
        "linear_key_head_dim": 128,
        "linear_num_key_heads": 16,
        "linear_num_value_heads": 16,
        "linear_value_head_dim": 128,
        "max_position_embeddings": 262144,
        "mlp_only_layers": [],
        "model_type": "qwen3_5_text",
        "mtp_num_hidden_layers": 1,
        "mtp_use_dedicated_embeddings": false,
        "num_attention_heads": 8,
        "num_hidden_layers": 4,
        "num_key_value_heads": 2,
        "rms_norm_eps": 1e-6,
        "tie_word_embeddings": true,
        "use_cache": true,
        "vocab_size": 248320,
        "mamba_ssm_dtype": "float32",
        "rope_parameters": {
          "mrope_interleaved": true,
          "mrope_section": [11, 11, 10],
          "rope_type": "default",
          "rope_theta": 10000000,
          "partial_rotary_factor": 0.25
        }
      },
      "tie_word_embeddings": true,
      "video_token_id": 248057,
      "vision_config": {
        "deepstack_visual_indexes": [],
        "depth": 12,
        "hidden_act": "gelu_pytorch_tanh",
        "hidden_size": 768,
        "in_channels": 3,
        "initializer_range": 0.02,
        "intermediate_size": 3072,
        "model_type": "qwen3_5",
        "num_heads": 12,
        "num_position_embeddings": 2304,
        "out_hidden_size": 1024,
        "patch_size": 16,
        "spatial_merge_size": 2,
        "temporal_patch_size": 2
      },
      "vision_end_token_id": 248054,
      "vision_start_token_id": 248053
    }
    "#;

    #[test]
    fn parses_local_checkpoint_shape() {
        let cfg: OvisOcr2Config = serde_json::from_str(CONFIG).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.text_config.layer_types.len(), 4);
        assert_eq!(cfg.vision_config.position_grid_size().unwrap(), 48);
        assert_eq!(cfg.vision_config.head_dim().unwrap(), 64);
        assert!(cfg.tie_word_embeddings());
        assert_eq!(cfg.text_config.hidden_act, Activation::Silu);
        assert_eq!(cfg.vision_config.hidden_act, Activation::GeluPytorchTanh);
    }

    #[test]
    fn processor_uses_official_runtime_bounds() {
        let cfg: OvisOcr2ImageProcessorConfig = serde_json::from_str(
            r#"{
              "size": {"shortest_edge": 65536, "longest_edge": 16777216},
              "patch_size": 16,
              "temporal_patch_size": 2,
              "merge_size": 2,
              "image_mean": [0.5, 0.5, 0.5],
              "image_std": [0.5, 0.5, 0.5]
            }"#,
        )
        .unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.runtime_pixel_bounds(), (448 * 448, 2880 * 2880));
        assert_eq!(cfg.rescale_factor, 1.0 / 255.0);
    }

    #[test]
    fn rejects_untied_checkpoint() {
        let mut cfg: OvisOcr2Config = serde_json::from_str(CONFIG).unwrap();
        cfg.tie_word_embeddings = false;
        cfg.text_config.tie_word_embeddings = false;
        assert!(cfg.validate().unwrap_err().to_string().contains("tied"));
    }
}
