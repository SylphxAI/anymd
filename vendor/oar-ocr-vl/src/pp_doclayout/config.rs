//! Configuration for PP-DocLayoutV2 and PP-DocLayoutV3.
//!
//! Mirrors `config.json` of the `PaddlePaddle/PP-DocLayoutV{2,3}_safetensors`
//! checkpoints. Defaults follow
//! `transformers/models/pp_doclayout_v{2,3}/configuration_pp_doclayout_v{2,3}.py`,
//! so fields the checkpoints omit still resolve to the upstream values.

use crate::error::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Which PP-DocLayout generation a checkpoint belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpDocLayoutVersion {
    /// PP-DocLayoutV2: RT-DETR detector plus a LayoutLM-style pointer network.
    V2,
    /// PP-DocLayoutV3: RT-DETR detector with an in-decoder order head.
    V3,
}

impl PpDocLayoutVersion {
    /// Resolves the version from the `model_type` field of `config.json`.
    pub fn from_model_type(model_type: &str) -> Result<Self, Error> {
        match model_type {
            "pp_doclayout_v2" => Ok(Self::V2),
            "pp_doclayout_v3" => Ok(Self::V3),
            other => Err(Error::Config {
                message: format!(
                    "PP-DocLayout: unsupported model_type '{other}', expected \
                     'pp_doclayout_v2' or 'pp_doclayout_v3'"
                ),
            }),
        }
    }
}

/// Backbone selection. Only the fields that vary between the two checkpoints
/// are read; the `L` stage geometry is baked into the backbone module.
#[derive(Debug, Clone, Deserialize)]
pub struct BackboneConfig {
    /// HGNetV2 architecture preset. Both checkpoints use `L`.
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Stage indices to return, e.g. `[1, 2, 3]` for stage2/3/4.
    #[serde(default = "default_return_idx")]
    pub return_idx: Vec<usize>,
}

fn default_arch() -> String {
    "L".to_string()
}

fn default_return_idx() -> Vec<usize> {
    vec![1, 2, 3]
}

/// Reading-order pointer network configuration (PP-DocLayoutV2 only).
#[derive(Debug, Clone, Deserialize)]
pub struct ReadingOrderConfig {
    #[serde(default = "default_ro_hidden_size")]
    pub hidden_size: usize,
    #[serde(default = "default_ro_heads")]
    pub num_attention_heads: usize,
    #[serde(default = "default_ro_layers")]
    pub num_hidden_layers: usize,
    #[serde(default = "default_ro_intermediate")]
    pub intermediate_size: usize,
    #[serde(default = "default_layer_norm_eps")]
    pub layer_norm_eps: f64,
    #[serde(default = "default_ro_max_positions")]
    pub max_position_embeddings: usize,
    #[serde(default = "default_ro_max_2d_positions")]
    pub max_2d_position_embeddings: usize,
    #[serde(default = "default_ro_type_vocab")]
    pub type_vocab_size: usize,
    #[serde(default = "default_ro_vocab")]
    pub vocab_size: usize,
    #[serde(default)]
    pub start_token_id: u32,
    #[serde(default = "default_ro_pad_token")]
    pub pad_token_id: u32,
    #[serde(default = "default_ro_end_token")]
    pub end_token_id: u32,
    #[serde(default = "default_ro_pred_token")]
    pub pred_token_id: u32,
    #[serde(default = "default_ro_coordinate_size")]
    pub coordinate_size: usize,
    #[serde(default = "default_ro_shape_size")]
    pub shape_size: usize,
    #[serde(default = "default_ro_num_classes")]
    pub num_classes: usize,
    #[serde(default = "default_ro_bias_embed_dim")]
    pub relation_bias_embed_dim: usize,
    #[serde(default = "default_ro_bias_theta")]
    pub relation_bias_theta: f64,
    #[serde(default = "default_ro_bias_scale")]
    pub relation_bias_scale: f64,
    #[serde(default = "default_global_pointer_head_size")]
    pub global_pointer_head_size: usize,
    #[serde(default = "crate::utils::default_true")]
    pub tril_mask: bool,
}

impl Default for ReadingOrderConfig {
    fn default() -> Self {
        Self {
            hidden_size: default_ro_hidden_size(),
            num_attention_heads: default_ro_heads(),
            num_hidden_layers: default_ro_layers(),
            intermediate_size: default_ro_intermediate(),
            layer_norm_eps: default_layer_norm_eps(),
            max_position_embeddings: default_ro_max_positions(),
            max_2d_position_embeddings: default_ro_max_2d_positions(),
            type_vocab_size: default_ro_type_vocab(),
            vocab_size: default_ro_vocab(),
            start_token_id: 0,
            pad_token_id: default_ro_pad_token(),
            end_token_id: default_ro_end_token(),
            pred_token_id: default_ro_pred_token(),
            coordinate_size: default_ro_coordinate_size(),
            shape_size: default_ro_shape_size(),
            num_classes: default_ro_num_classes(),
            relation_bias_embed_dim: default_ro_bias_embed_dim(),
            relation_bias_theta: default_ro_bias_theta(),
            relation_bias_scale: default_ro_bias_scale(),
            global_pointer_head_size: default_global_pointer_head_size(),
            tril_mask: true,
        }
    }
}

fn default_ro_hidden_size() -> usize {
    512
}
fn default_ro_heads() -> usize {
    8
}
fn default_ro_layers() -> usize {
    6
}
fn default_ro_intermediate() -> usize {
    2048
}
fn default_ro_max_positions() -> usize {
    514
}
fn default_ro_max_2d_positions() -> usize {
    1024
}
fn default_ro_type_vocab() -> usize {
    1
}
fn default_ro_vocab() -> usize {
    4
}
fn default_ro_pad_token() -> u32 {
    1
}
fn default_ro_end_token() -> u32 {
    2
}
fn default_ro_pred_token() -> u32 {
    3
}
fn default_ro_coordinate_size() -> usize {
    171
}
fn default_ro_shape_size() -> usize {
    170
}
fn default_ro_num_classes() -> usize {
    20
}
fn default_ro_bias_embed_dim() -> usize {
    16
}
fn default_ro_bias_theta() -> f64 {
    10000.0
}
fn default_ro_bias_scale() -> f64 {
    100.0
}
fn default_global_pointer_head_size() -> usize {
    64
}

/// Top-level PP-DocLayout configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct PpDocLayoutConfig {
    pub model_type: String,
    #[serde(default)]
    pub backbone_config: Option<BackboneConfig>,

    #[serde(default = "default_d_model")]
    pub d_model: usize,
    #[serde(default = "default_encoder_hidden_dim")]
    pub encoder_hidden_dim: usize,
    #[serde(default = "default_encoder_in_channels")]
    pub encoder_in_channels: Vec<usize>,
    /// V2 spells this `feat_strides`, V3 `feature_strides`.
    #[serde(default, alias = "feature_strides")]
    pub feat_strides: Option<Vec<usize>>,
    #[serde(default = "default_encoder_layers")]
    pub encoder_layers: usize,
    #[serde(default = "default_ffn_dim")]
    pub encoder_ffn_dim: usize,
    #[serde(default = "default_attention_heads")]
    pub encoder_attention_heads: usize,
    #[serde(default = "default_encode_proj_layers")]
    pub encode_proj_layers: Vec<usize>,
    #[serde(default = "default_positional_encoding_temperature")]
    pub positional_encoding_temperature: f64,
    #[serde(default = "default_encoder_activation")]
    pub encoder_activation_function: String,
    #[serde(default = "default_activation")]
    pub activation_function: String,
    /// Pre-LN encoder layers. Only `false` (post-LN) is implemented, which is
    /// what both checkpoints ship; loading rejects `true` rather than quietly
    /// running the wrong layer order.
    #[serde(default)]
    pub normalize_before: bool,
    #[serde(default = "default_hidden_expansion")]
    pub hidden_expansion: f64,

    #[serde(default = "default_num_queries")]
    pub num_queries: usize,
    #[serde(default = "default_decoder_in_channels")]
    pub decoder_in_channels: Vec<usize>,
    #[serde(default = "default_ffn_dim")]
    pub decoder_ffn_dim: usize,
    #[serde(default = "default_num_feature_levels")]
    pub num_feature_levels: usize,
    #[serde(default = "default_decoder_n_points")]
    pub decoder_n_points: usize,
    #[serde(default = "default_decoder_layers")]
    pub decoder_layers: usize,
    #[serde(default = "default_attention_heads")]
    pub decoder_attention_heads: usize,
    #[serde(default = "default_decoder_activation")]
    pub decoder_activation_function: String,

    #[serde(default = "default_layer_norm_eps")]
    pub layer_norm_eps: f64,
    #[serde(default = "default_batch_norm_eps")]
    pub batch_norm_eps: f64,

    /// Class id to label name. The checkpoints map several ids to the same
    /// name (for example both `footer` entries), which is intentional.
    pub id2label: HashMap<String, String>,
    /// Per-class score thresholds (V2). V3 uses a single threshold.
    #[serde(default)]
    pub class_thresholds: Option<Vec<f32>>,
    /// Detection class id to reading-order category (V2).
    #[serde(default)]
    pub class_order: Option<Vec<usize>>,
    /// Reading-order pointer network (V2).
    #[serde(default)]
    pub reading_order_config: Option<ReadingOrderConfig>,

    /// Global pointer head size for the V3 order head.
    #[serde(default = "default_global_pointer_head_size")]
    pub global_pointer_head_size: usize,
    /// Channel widths of the V3 mask feature head.
    #[serde(default = "default_mask_feature_channels")]
    pub mask_feature_channels: Vec<usize>,
    /// Channel width of the V3 stage1 (stride 4) feature map.
    #[serde(default = "default_x4_feat_dim")]
    pub x4_feat_dim: usize,
    /// Number of V3 mask prototypes.
    #[serde(default = "default_num_prototypes")]
    pub num_prototypes: usize,
    /// Whether V3 derives the decoder's initial boxes from the predicted
    /// masks. Upstream defaults to `true`.
    #[serde(default = "crate::utils::default_true")]
    pub mask_enhanced: bool,
}

fn default_d_model() -> usize {
    256
}
fn default_encoder_hidden_dim() -> usize {
    256
}
fn default_encoder_in_channels() -> Vec<usize> {
    vec![512, 1024, 2048]
}
fn default_encoder_layers() -> usize {
    1
}
fn default_ffn_dim() -> usize {
    1024
}
fn default_attention_heads() -> usize {
    8
}
fn default_encode_proj_layers() -> Vec<usize> {
    vec![2]
}
fn default_positional_encoding_temperature() -> f64 {
    10000.0
}
fn default_encoder_activation() -> String {
    "gelu".to_string()
}
fn default_activation() -> String {
    "silu".to_string()
}
fn default_hidden_expansion() -> f64 {
    1.0
}
fn default_num_queries() -> usize {
    300
}
fn default_decoder_in_channels() -> Vec<usize> {
    vec![256, 256, 256]
}
fn default_num_feature_levels() -> usize {
    3
}
fn default_decoder_n_points() -> usize {
    4
}
fn default_decoder_layers() -> usize {
    6
}
fn default_decoder_activation() -> String {
    "relu".to_string()
}
fn default_layer_norm_eps() -> f64 {
    1e-5
}
fn default_batch_norm_eps() -> f64 {
    1e-5
}
fn default_mask_feature_channels() -> Vec<usize> {
    vec![64, 64]
}
fn default_x4_feat_dim() -> usize {
    128
}
fn default_num_prototypes() -> usize {
    32
}

impl PpDocLayoutConfig {
    /// Loads `config.json` from a checkpoint directory entry.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let cfg: Self = crate::utils::load_json_config(path, "PP-DocLayout", "config.json")?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Which generation this configuration describes.
    pub fn version(&self) -> Result<PpDocLayoutVersion, Error> {
        PpDocLayoutVersion::from_model_type(&self.model_type)
    }

    /// Number of detection classes.
    pub fn num_labels(&self) -> usize {
        self.id2label.len()
    }

    /// Feature-map strides, defaulting to the RT-DETR `[8, 16, 32]`.
    pub fn strides(&self) -> Vec<usize> {
        self.feat_strides.clone().unwrap_or_else(|| vec![8, 16, 32])
    }

    /// Backbone stage indices to return.
    pub fn backbone_return_idx(&self) -> Vec<usize> {
        self.backbone_config
            .as_ref()
            .map(|b| b.return_idx.clone())
            .unwrap_or_else(default_return_idx)
    }

    /// Label for a class id, or `unknown` when the id is not in `id2label`.
    pub fn label(&self, class_id: usize) -> &str {
        self.id2label
            .get(&class_id.to_string())
            .map(String::as_str)
            .unwrap_or("unknown")
    }

    fn validate(&self) -> Result<(), Error> {
        let version = self.version()?;

        if let Some(backbone) = &self.backbone_config
            && backbone.arch != "L"
        {
            return Err(Error::Config {
                message: format!(
                    "PP-DocLayout: unsupported backbone arch '{}', only 'L' is implemented",
                    backbone.arch
                ),
            });
        }

        if self.normalize_before {
            return Err(Error::Config {
                message: "PP-DocLayout: normalize_before=true (pre-LN encoder) is not implemented"
                    .to_string(),
            });
        }

        if self.id2label.is_empty() {
            return Err(Error::Config {
                message: "PP-DocLayout: config.json has an empty id2label map".to_string(),
            });
        }

        if let Some(thresholds) = &self.class_thresholds
            && thresholds.len() != self.num_labels()
        {
            return Err(Error::Config {
                message: format!(
                    "PP-DocLayout: class_thresholds has {} entries but there are {} labels",
                    thresholds.len(),
                    self.num_labels()
                ),
            });
        }

        if version == PpDocLayoutVersion::V2 {
            let Some(class_order) = &self.class_order else {
                return Err(Error::Config {
                    message: "PP-DocLayoutV2: config.json is missing class_order".to_string(),
                });
            };
            if class_order.len() != self.num_labels() {
                return Err(Error::Config {
                    message: format!(
                        "PP-DocLayoutV2: class_order has {} entries but there are {} labels",
                        class_order.len(),
                        self.num_labels()
                    ),
                });
            }
        }

        if self.encoder_in_channels.len() != self.num_feature_levels {
            return Err(Error::Config {
                message: format!(
                    "PP-DocLayout: encoder_in_channels has {} entries but num_feature_levels is {}",
                    self.encoder_in_channels.len(),
                    self.num_feature_levels
                ),
            });
        }

        Ok(())
    }

    /// Reading-order configuration, falling back to upstream defaults.
    pub fn reading_order(&self) -> ReadingOrderConfig {
        self.reading_order_config.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_json(extra: &str) -> String {
        format!(
            r#"{{
                "model_type": "pp_doclayout_v2",
                "backbone_config": {{"arch": "L", "return_idx": [1, 2, 3]}},
                "feat_strides": [8, 16, 32],
                "id2label": {{"0": "text", "1": "table"}},
                "class_thresholds": [0.5, 0.4],
                "class_order": [2, 12],
                "reading_order_config": {{"hidden_size": 512, "num_classes": 20}}
                {extra}
            }}"#
        )
    }

    #[test]
    fn parses_a_v2_configuration() {
        let cfg: PpDocLayoutConfig = serde_json::from_str(&v2_json("")).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.version().unwrap(), PpDocLayoutVersion::V2);
        assert_eq!(cfg.num_labels(), 2);
        assert_eq!(cfg.strides(), vec![8, 16, 32]);
        assert_eq!(cfg.backbone_return_idx(), vec![1, 2, 3]);
        assert_eq!(cfg.label(1), "table");
        assert_eq!(cfg.label(9), "unknown");
        assert_eq!(cfg.num_queries, 300);
        assert_eq!(cfg.decoder_layers, 6);
        assert_eq!(cfg.reading_order().global_pointer_head_size, 64);
    }

    /// V3 spells the stride list `feature_strides` and returns four backbone
    /// stages, the first of which feeds the mask branch.
    #[test]
    fn parses_a_v3_configuration() {
        let json = r#"{
            "model_type": "pp_doclayout_v3",
            "backbone_config": {"arch": "L", "return_idx": [0, 1, 2, 3]},
            "feature_strides": [8, 16, 32],
            "id2label": {"0": "text"},
            "mask_feature_channels": [64, 64],
            "x4_feat_dim": 128
        }"#;
        let cfg: PpDocLayoutConfig = serde_json::from_str(json).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.version().unwrap(), PpDocLayoutVersion::V3);
        assert_eq!(cfg.strides(), vec![8, 16, 32]);
        assert_eq!(cfg.backbone_return_idx(), vec![0, 1, 2, 3]);
        assert_eq!(cfg.num_prototypes, 32);
        assert!(cfg.mask_enhanced);
    }

    #[test]
    fn rejects_unknown_model_types() {
        assert!(PpDocLayoutVersion::from_model_type("rt_detr").is_err());
    }

    #[test]
    fn rejects_a_v2_configuration_without_class_order() {
        let json = r#"{
            "model_type": "pp_doclayout_v2",
            "id2label": {"0": "text"},
            "class_thresholds": [0.5]
        }"#;
        let cfg: PpDocLayoutConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_mismatched_threshold_counts() {
        let json = r#"{
            "model_type": "pp_doclayout_v3",
            "id2label": {"0": "text", "1": "table"},
            "class_thresholds": [0.5]
        }"#;
        let cfg: PpDocLayoutConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.validate().is_err());
    }

    /// Post-LN is baked into the encoder, so a pre-LN checkpoint must be
    /// refused rather than silently mis-run.
    #[test]
    fn rejects_pre_layer_norm_encoders() {
        let json = r#"{
            "model_type": "pp_doclayout_v3",
            "id2label": {"0": "text"},
            "normalize_before": true
        }"#;
        let cfg: PpDocLayoutConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_unsupported_backbone_architectures() {
        let json = r#"{
            "model_type": "pp_doclayout_v3",
            "backbone_config": {"arch": "X"},
            "id2label": {"0": "text"}
        }"#;
        let cfg: PpDocLayoutConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.validate().is_err());
    }
}
