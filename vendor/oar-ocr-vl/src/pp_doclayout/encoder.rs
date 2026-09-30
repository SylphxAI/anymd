//! RT-DETR hybrid encoder: AIFI on the coarsest level, then a top-down FPN and
//! a bottom-up PAN over the projected backbone features.
//!
//! Ported from `transformers/models/rt_detr/modeling_rt_detr.py`
//! (`RTDetrHybridEncoder` and friends). The checkpoints store the AIFI stack
//! under `model.encoder.encoder.*`, which is the name used here.

use super::config::PpDocLayoutConfig;
use super::err::infer_err;
use super::layers::{Activation, ConvNormLayer};
use super::mask_head::MaskFeatureHead;
use crate::error::Error;
use candle_core::{DType, Device, Tensor};
use candle_nn::{LayerNorm, Linear, Module, VarBuilder, layer_norm, linear};

/// Multi-head self-attention where the position embedding is added to the
/// query and key inputs but not to the values.
#[derive(Debug)]
struct SelfAttention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    out_proj: Linear,
    num_heads: usize,
    head_dim: usize,
    scaling: f64,
}

impl SelfAttention {
    fn load(hidden_size: usize, num_heads: usize, vb: VarBuilder) -> Result<Self, Error> {
        let head_dim = hidden_size / num_heads;
        Ok(Self {
            q_proj: linear(hidden_size, hidden_size, vb.pp("q_proj"))
                .map_err(|e| infer_err("load encoder q_proj", e))?,
            k_proj: linear(hidden_size, hidden_size, vb.pp("k_proj"))
                .map_err(|e| infer_err("load encoder k_proj", e))?,
            v_proj: linear(hidden_size, hidden_size, vb.pp("v_proj"))
                .map_err(|e| infer_err("load encoder v_proj", e))?,
            out_proj: linear(hidden_size, hidden_size, vb.pp("out_proj"))
                .map_err(|e| infer_err("load encoder out_proj", e))?,
            num_heads,
            head_dim,
            scaling: (head_dim as f64).powf(-0.5),
        })
    }

    fn forward(&self, hidden: &Tensor, position: Option<&Tensor>) -> Result<Tensor, Error> {
        let (batch, seq_len, _) = hidden
            .dims3()
            .map_err(|e| infer_err("encoder attention shape", e))?;
        let query_key_input = match position {
            Some(pos) => hidden
                .broadcast_add(pos)
                .map_err(|e| infer_err("encoder position add", e))?,
            None => hidden.clone(),
        };

        let split = |t: Tensor, what: &'static str| -> Result<Tensor, Error> {
            t.reshape((batch, seq_len, self.num_heads, self.head_dim))
                .and_then(|t| t.transpose(1, 2))
                .and_then(|t| t.contiguous())
                .map_err(|e| infer_err(what, e))
        };

        let query = split(
            self.q_proj
                .forward(&query_key_input)
                .map_err(|e| infer_err("encoder q projection", e))?,
            "encoder q reshape",
        )?;
        let key = split(
            self.k_proj
                .forward(&query_key_input)
                .map_err(|e| infer_err("encoder k projection", e))?,
            "encoder k reshape",
        )?;
        let value = split(
            self.v_proj
                .forward(hidden)
                .map_err(|e| infer_err("encoder v projection", e))?,
            "encoder v reshape",
        )?;

        let scores = (query
            .matmul(
                &key.transpose(2, 3)
                    .and_then(|t| t.contiguous())
                    .map_err(|e| infer_err("encoder key transpose", e))?,
            )
            .map_err(|e| infer_err("encoder attention scores", e))?
            * self.scaling)
            .map_err(|e| infer_err("encoder attention scaling", e))?;
        let probs = candle_nn::ops::softmax_last_dim(&scores)
            .map_err(|e| infer_err("encoder attention softmax", e))?;
        let context = probs
            .matmul(&value)
            .map_err(|e| infer_err("encoder attention context", e))?
            .transpose(1, 2)
            .map_err(|e| infer_err("encoder context transpose", e))?
            .reshape((batch, seq_len, self.num_heads * self.head_dim))
            .map_err(|e| infer_err("encoder context reshape", e))?;
        self.out_proj
            .forward(&context)
            .map_err(|e| infer_err("encoder output projection", e))
    }
}

/// One post-norm transformer encoder layer.
#[derive(Debug)]
struct EncoderLayer {
    self_attn: SelfAttention,
    self_attn_layer_norm: LayerNorm,
    fc1: Linear,
    fc2: Linear,
    final_layer_norm: LayerNorm,
    activation: Activation,
}

impl EncoderLayer {
    fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let hidden = cfg.encoder_hidden_dim;
        Ok(Self {
            self_attn: SelfAttention::load(
                hidden,
                cfg.encoder_attention_heads,
                vb.pp("self_attn"),
            )?,
            self_attn_layer_norm: layer_norm(
                hidden,
                cfg.layer_norm_eps,
                vb.pp("self_attn_layer_norm"),
            )
            .map_err(|e| infer_err("load encoder self_attn_layer_norm", e))?,
            fc1: linear(hidden, cfg.encoder_ffn_dim, vb.pp("fc1"))
                .map_err(|e| infer_err("load encoder fc1", e))?,
            fc2: linear(cfg.encoder_ffn_dim, hidden, vb.pp("fc2"))
                .map_err(|e| infer_err("load encoder fc2", e))?,
            final_layer_norm: layer_norm(hidden, cfg.layer_norm_eps, vb.pp("final_layer_norm"))
                .map_err(|e| infer_err("load encoder final_layer_norm", e))?,
            activation: Activation::parse(&cfg.encoder_activation_function)?,
        })
    }

    fn forward(&self, hidden: &Tensor, position: Option<&Tensor>) -> Result<Tensor, Error> {
        let attn = self.self_attn.forward(hidden, position)?;
        let hidden = (hidden + attn).map_err(|e| infer_err("encoder attention residual", e))?;
        let hidden = self
            .self_attn_layer_norm
            .forward(&hidden)
            .map_err(|e| infer_err("encoder attention norm", e))?;

        let ffn = self
            .fc1
            .forward(&hidden)
            .map_err(|e| infer_err("encoder fc1", e))?;
        let ffn = self.activation.apply(&ffn)?;
        let ffn = self
            .fc2
            .forward(&ffn)
            .map_err(|e| infer_err("encoder fc2", e))?;
        let hidden = (hidden + ffn).map_err(|e| infer_err("encoder ffn residual", e))?;
        self.final_layer_norm
            .forward(&hidden)
            .map_err(|e| infer_err("encoder ffn norm", e))
    }
}

/// Attention-based intra-scale feature interaction over a single feature map.
#[derive(Debug)]
struct AifiLayer {
    layers: Vec<EncoderLayer>,
    embed_dim: usize,
    temperature: f64,
}

impl AifiLayer {
    fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let layers_vb = vb.pp("layers");
        let mut layers = Vec::with_capacity(cfg.encoder_layers);
        for i in 0..cfg.encoder_layers {
            layers.push(EncoderLayer::load(cfg, layers_vb.pp(i))?);
        }
        Ok(Self {
            layers,
            embed_dim: cfg.encoder_hidden_dim,
            temperature: cfg.positional_encoding_temperature,
        })
    }

    fn forward(&self, feature_map: &Tensor) -> Result<Tensor, Error> {
        let (batch, channels, height, width) = feature_map
            .dims4()
            .map_err(|e| infer_err("aifi input shape", e))?;
        let mut hidden = feature_map
            .flatten_from(2)
            .and_then(|t| t.permute((0, 2, 1)))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("aifi flatten", e))?;

        let position = sine_position_embedding(
            height,
            width,
            self.embed_dim,
            self.temperature,
            feature_map.device(),
            feature_map.dtype(),
        )?;

        for layer in &self.layers {
            hidden = layer.forward(&hidden, Some(&position))?;
        }

        hidden
            .permute((0, 2, 1))
            .and_then(|t| t.reshape((batch, channels, height, width)))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("aifi reshape", e))
    }
}

/// 2D sinusoidal position embedding laid out as `[sin_h | cos_h | sin_w |
/// cos_w]` in row-major (height-outer) order, shaped `(1, height*width, dim)`.
fn sine_position_embedding(
    height: usize,
    width: usize,
    embed_dim: usize,
    temperature: f64,
    device: &Device,
    dtype: DType,
) -> Result<Tensor, Error> {
    if !embed_dim.is_multiple_of(4) {
        return Err(Error::Config {
            message: format!("PP-DocLayout: encoder_hidden_dim {embed_dim} must be divisible by 4"),
        });
    }
    let pos_dim = embed_dim / 4;
    // Torch builds these in f64 before casting, so do the same to stay
    // bit-comparable at the tail frequencies.
    let omega: Vec<f64> = (0..pos_dim)
        .map(|i| 1.0 / temperature.powf(i as f64 / pos_dim as f64))
        .collect();

    let mut data = vec![0f64; height * width * embed_dim];
    for h in 0..height {
        for w in 0..width {
            let base = (h * width + w) * embed_dim;
            for (k, &om) in omega.iter().enumerate() {
                let ph = h as f64 * om;
                let pw = w as f64 * om;
                data[base + k] = ph.sin();
                data[base + pos_dim + k] = ph.cos();
                data[base + 2 * pos_dim + k] = pw.sin();
                data[base + 3 * pos_dim + k] = pw.cos();
            }
        }
    }

    // Build and downcast on CPU: Metal has no F64, so materializing the f64
    // table on the target device fails there. Casting before upload keeps the
    // values identical on every backend.
    Tensor::from_vec(data, (1, height * width, embed_dim), &Device::Cpu)
        .and_then(|t| t.to_dtype(dtype))
        .and_then(|t| t.to_device(device))
        .map_err(|e| infer_err("build position embedding", e))
}

/// RepVGG block: a 3x3 and a 1x1 branch summed, then activated.
#[derive(Debug)]
struct RepVggBlock {
    conv1: ConvNormLayer,
    conv2: ConvNormLayer,
    activation: Activation,
}

impl RepVggBlock {
    fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let hidden = (cfg.encoder_hidden_dim as f64 * cfg.hidden_expansion) as usize;
        Ok(Self {
            conv1: ConvNormLayer::load(
                hidden,
                hidden,
                3,
                1,
                Some(1),
                Activation::Identity,
                cfg.batch_norm_eps,
                vb.pp("conv1"),
            )?,
            conv2: ConvNormLayer::load(
                hidden,
                hidden,
                1,
                1,
                Some(0),
                Activation::Identity,
                cfg.batch_norm_eps,
                vb.pp("conv2"),
            )?,
            activation: Activation::parse(&cfg.activation_function)?,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let y = (self.conv1.forward(x)? + self.conv2.forward(x)?)
            .map_err(|e| infer_err("repvgg branch sum", e))?;
        self.activation.apply(&y)
    }
}

/// Cross-stage-partial layer with three RepVGG bottlenecks.
#[derive(Debug)]
struct CspRepLayer {
    conv1: ConvNormLayer,
    conv2: ConvNormLayer,
    bottlenecks: Vec<RepVggBlock>,
}

impl CspRepLayer {
    const NUM_BLOCKS: usize = 3;

    fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let in_channels = cfg.encoder_hidden_dim * 2;
        let out_channels = cfg.encoder_hidden_dim;
        let hidden = (out_channels as f64 * cfg.hidden_expansion) as usize;
        if hidden != out_channels {
            return Err(Error::Config {
                message: "PP-DocLayout: hidden_expansion != 1.0 is not supported".to_string(),
            });
        }
        let activation = Activation::parse(&cfg.activation_function)?;
        let bottlenecks_vb = vb.pp("bottlenecks");
        let mut bottlenecks = Vec::with_capacity(Self::NUM_BLOCKS);
        for i in 0..Self::NUM_BLOCKS {
            bottlenecks.push(RepVggBlock::load(cfg, bottlenecks_vb.pp(i))?);
        }
        Ok(Self {
            conv1: ConvNormLayer::load(
                in_channels,
                hidden,
                1,
                1,
                None,
                activation,
                cfg.batch_norm_eps,
                vb.pp("conv1"),
            )?,
            conv2: ConvNormLayer::load(
                in_channels,
                hidden,
                1,
                1,
                None,
                activation,
                cfg.batch_norm_eps,
                vb.pp("conv2"),
            )?,
            bottlenecks,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let mut branch = self.conv1.forward(x)?;
        for block in &self.bottlenecks {
            branch = block.forward(&branch)?;
        }
        let other = self.conv2.forward(x)?;
        (branch + other).map_err(|e| infer_err("csp merge", e))
    }
}

/// The full hybrid encoder.
#[derive(Debug)]
pub(super) struct HybridEncoder {
    aifi: Vec<AifiLayer>,
    encode_proj_layers: Vec<usize>,
    lateral_convs: Vec<ConvNormLayer>,
    fpn_blocks: Vec<CspRepLayer>,
    downsample_convs: Vec<ConvNormLayer>,
    pan_blocks: Vec<CspRepLayer>,
    mask_head: Option<MaskFeatureHead>,
}

impl HybridEncoder {
    /// `with_mask_head` loads the V3 mask branch.
    pub(super) fn load(
        cfg: &PpDocLayoutConfig,
        with_mask_head: bool,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let num_stages = cfg.encoder_in_channels.len().saturating_sub(1);
        let hidden = cfg.encoder_hidden_dim;
        let activation = Activation::parse(&cfg.activation_function)?;

        // Upstream calls this stack `aifi`; the checkpoints store it as
        // `encoder`, which is the name we have to load from.
        let aifi_vb = vb.pp("encoder");
        let mut aifi = Vec::with_capacity(cfg.encode_proj_layers.len());
        for i in 0..cfg.encode_proj_layers.len() {
            aifi.push(AifiLayer::load(cfg, aifi_vb.pp(i))?);
        }

        let lateral_vb = vb.pp("lateral_convs");
        let fpn_vb = vb.pp("fpn_blocks");
        let mut lateral_convs = Vec::with_capacity(num_stages);
        let mut fpn_blocks = Vec::with_capacity(num_stages);
        for i in 0..num_stages {
            lateral_convs.push(ConvNormLayer::load(
                hidden,
                hidden,
                1,
                1,
                None,
                activation,
                cfg.batch_norm_eps,
                lateral_vb.pp(i),
            )?);
            fpn_blocks.push(CspRepLayer::load(cfg, fpn_vb.pp(i))?);
        }

        let downsample_vb = vb.pp("downsample_convs");
        let pan_vb = vb.pp("pan_blocks");
        let mut downsample_convs = Vec::with_capacity(num_stages);
        let mut pan_blocks = Vec::with_capacity(num_stages);
        for i in 0..num_stages {
            downsample_convs.push(ConvNormLayer::load(
                hidden,
                hidden,
                3,
                2,
                None,
                activation,
                cfg.batch_norm_eps,
                downsample_vb.pp(i),
            )?);
            pan_blocks.push(CspRepLayer::load(cfg, pan_vb.pp(i))?);
        }

        Ok(Self {
            aifi,
            encode_proj_layers: cfg.encode_proj_layers.clone(),
            lateral_convs,
            fpn_blocks,
            downsample_convs,
            pan_blocks,
            mask_head: with_mask_head
                .then(|| MaskFeatureHead::load(cfg, vb.clone()))
                .transpose()?,
        })
    }

    /// Runs the V3 mask branch over the fused levels, if this encoder has one.
    pub(super) fn mask_features(
        &self,
        pan: &[Tensor],
        x4: &Tensor,
    ) -> Result<Option<Tensor>, Error> {
        self.mask_head
            .as_ref()
            .map(|head| head.forward(pan, x4))
            .transpose()
    }

    /// Fuses the projected backbone features, returning one map per level in
    /// fine-to-coarse order.
    pub(super) fn forward(&self, features: &[Tensor]) -> Result<Vec<Tensor>, Error> {
        let mut features = features.to_vec();
        for (i, &level) in self.encode_proj_layers.iter().enumerate() {
            let Some(feature) = features.get(level) else {
                return Err(Error::Config {
                    message: format!(
                        "PP-DocLayout: encode_proj_layers references level {level} but only \
                         {} feature levels are available",
                        features.len()
                    ),
                });
            };
            features[level] = self.aifi[i].forward(feature)?;
        }

        let num_stages = self.lateral_convs.len();
        let mut fpn = vec![features[features.len() - 1].clone()];
        for (idx, (lateral, block)) in self
            .lateral_convs
            .iter()
            .zip(self.fpn_blocks.iter())
            .enumerate()
        {
            let backbone_feature = &features[num_stages - idx - 1];
            let top = lateral.forward(&fpn[fpn.len() - 1])?;
            let last = fpn.len() - 1;
            fpn[last] = top.clone();

            let (_, _, height, width) = top
                .dims4()
                .map_err(|e| infer_err("fpn upsample shape", e))?;
            let upsampled = top
                .upsample_nearest2d(height * 2, width * 2)
                .map_err(|e| infer_err("fpn upsample", e))?;
            let fused = Tensor::cat(&[&upsampled, backbone_feature], 1)
                .map_err(|e| infer_err("fpn concat", e))?;
            fpn.push(block.forward(&fused)?);
        }
        fpn.reverse();

        let mut pan = vec![fpn[0].clone()];
        for (idx, (downsample, block)) in self
            .downsample_convs
            .iter()
            .zip(self.pan_blocks.iter())
            .enumerate()
        {
            let downsampled = downsample.forward(&pan[pan.len() - 1])?;
            let fused = Tensor::cat(&[&downsampled, &fpn[idx + 1]], 1)
                .map_err(|e| infer_err("pan concat", e))?;
            pan.push(block.forward(&fused)?);
        }

        Ok(pan)
    }
}
