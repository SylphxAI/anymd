//! Qwen3.5 text decoder used by OvisOCR2.
//!
//! Qwen3.5 alternates three Gated DeltaNet layers with one full-attention
//! layer. Its decoder RMSNorm checkpoints are zero-centred (`1 + weight`),
//! while Gated DeltaNet's internal gated RMSNorm is conventionally centred at
//! one. Its multimodal RoPE frequencies are interleaved T/H/W.

use super::config::OvisOcr2TextConfig;
use super::gated_delta::gated_delta_rule;
use crate::attention::{RotaryEmbedding, flash_attention, scaled_dot_product_attention_gqa};
use crate::error::Error;
use crate::kv_trim::TrimmableKvCache;
use crate::utils::{candle_to_ocr_inference, rotate_half};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{
    Conv1d, Conv1dConfig, Embedding, Linear, Module, RmsNorm, VarBuilder, embedding,
    linear_no_bias, rms_norm,
};
use std::cell::RefCell;

const MODEL_NAME: &str = "OvisOCR2";

#[derive(Debug, Clone)]
struct AdditiveRmsNorm {
    weight: Tensor,
    eps: f64,
}

impl AdditiveRmsNorm {
    fn load(dim: usize, eps: f64, vb: VarBuilder) -> Result<Self, Error> {
        let weight = vb
            .get(dim, "weight")
            .and_then(|weight| weight.to_dtype(DType::F32))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load additive RMSNorm", e))?;
        Ok(Self { weight, eps })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor, Error> {
        let dtype = xs.dtype();
        let xs = xs
            .to_dtype(DType::F32)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "RMSNorm input cast", e))?;
        let variance = xs
            .sqr()
            .and_then(|xs| xs.mean_keepdim(D::Minus1))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "RMSNorm variance", e))?;
        let normalized = xs
            .broadcast_div(
                &(variance + self.eps)
                    .and_then(|variance| variance.sqrt())
                    .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "RMSNorm rsqrt", e))?,
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "RMSNorm normalize", e))?;
        let scale = (&self.weight + 1.0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "RMSNorm scale", e))?;
        normalized
            .broadcast_mul(&scale)
            .and_then(|xs| xs.to_dtype(dtype))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "RMSNorm output", e))
    }
}

#[derive(Debug, Clone)]
struct OvisMlp {
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
}

impl OvisMlp {
    fn load(cfg: &OvisOcr2TextConfig, vb: VarBuilder) -> Result<Self, Error> {
        let gate_proj = linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("gate_proj"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load MLP gate_proj", e))?;
        let up_proj = linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("up_proj"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load MLP up_proj", e))?;
        let down_proj = linear_no_bias(cfg.intermediate_size, cfg.hidden_size, vb.pp("down_proj"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load MLP down_proj", e))?;
        Ok(Self {
            gate_proj,
            up_proj,
            down_proj,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor, Error> {
        let gate = self
            .gate_proj
            .forward(xs)
            .and_then(|gate| candle_nn::ops::silu(&gate))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "MLP gate", e))?;
        let up = self
            .up_proj
            .forward(xs)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "MLP up", e))?;
        self.down_proj
            .forward(
                &(&gate * &up)
                    .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "MLP gate product", e))?,
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "MLP down", e))
    }
}

#[derive(Debug)]
struct GatedDeltaNet {
    in_proj_qkv: Linear,
    in_proj_z: Linear,
    in_proj_b: Linear,
    in_proj_a: Linear,
    conv1d: Conv1d,
    decode_conv_weight: Tensor,
    dt_bias: Tensor,
    neg_a: Tensor,
    norm: RmsNorm,
    out_proj: Linear,
    num_key_heads: usize,
    num_value_heads: usize,
    key_head_dim: usize,
    value_head_dim: usize,
    conv_kernel_size: usize,
    conv_state: RefCell<Option<Tensor>>,
    recurrent_state: RefCell<Option<Tensor>>,
}

fn cached_depthwise_conv_step(
    state: &Tensor,
    mixed: &Tensor,
    weight: &Tensor,
    kernel_size: usize,
) -> candle_core::Result<(Tensor, Tensor)> {
    let tail = state.narrow(2, 1, kernel_size - 1)?;
    let new_state = Tensor::cat(&[&tail, mixed], 2)?;
    let output = match mixed.dtype() {
        DType::BF16 | DType::F16 => new_state
            .to_dtype(DType::F32)?
            .broadcast_mul(weight)?
            .sum_keepdim(2)?
            .to_dtype(mixed.dtype())?,
        _ => new_state.broadcast_mul(weight)?.sum_keepdim(2)?,
    };
    Ok((output, new_state))
}

impl GatedDeltaNet {
    fn load(cfg: &OvisOcr2TextConfig, vb: VarBuilder) -> Result<Self, Error> {
        let key_dim = cfg.linear_num_key_heads * cfg.linear_key_head_dim;
        let value_dim = cfg.linear_num_value_heads * cfg.linear_value_head_dim;
        let conv_dim = key_dim * 2 + value_dim;
        if !cfg
            .linear_num_value_heads
            .is_multiple_of(cfg.linear_num_key_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2: linear_num_value_heads ({}) must be divisible by linear_num_key_heads ({})",
                    cfg.linear_num_value_heads, cfg.linear_num_key_heads
                ),
            });
        }
        if cfg.linear_key_head_dim != cfg.linear_value_head_dim {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2 currently requires equal Gated DeltaNet key/value head dims, got {}/{}",
                    cfg.linear_key_head_dim, cfg.linear_value_head_dim
                ),
            });
        }

        let in_proj_qkv = linear_no_bias(cfg.hidden_size, conv_dim, vb.pp("in_proj_qkv"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN in_proj_qkv", e))?;
        let in_proj_z = linear_no_bias(cfg.hidden_size, value_dim, vb.pp("in_proj_z"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN in_proj_z", e))?;
        let in_proj_b = linear_no_bias(
            cfg.hidden_size,
            cfg.linear_num_value_heads,
            vb.pp("in_proj_b"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN in_proj_b", e))?;
        let in_proj_a = linear_no_bias(
            cfg.hidden_size,
            cfg.linear_num_value_heads,
            vb.pp("in_proj_a"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN in_proj_a", e))?;
        let conv_weight = vb
            .get((conv_dim, 1, cfg.linear_conv_kernel_dim), "conv1d.weight")
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN conv1d", e))?;
        let decode_conv_weight = conv_weight
            .to_dtype(DType::F32)
            .and_then(|weight| weight.squeeze(1))
            .and_then(|weight| weight.unsqueeze(0))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN decode conv weight", e))?;
        let conv1d = Conv1d::new(
            conv_weight,
            None,
            Conv1dConfig {
                padding: cfg.linear_conv_kernel_dim.saturating_sub(1),
                groups: conv_dim,
                ..Default::default()
            },
        );
        let dt_bias = vb
            .get(cfg.linear_num_value_heads, "dt_bias")
            .and_then(|x| x.to_dtype(DType::F32))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN dt_bias", e))?;
        let neg_a = vb
            .get(cfg.linear_num_value_heads, "A_log")
            .and_then(|x| x.to_dtype(DType::F32))
            .and_then(|x| x.exp())
            .and_then(|x| x.neg())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN A_log", e))?;
        // Qwen3_5RMSNormGated initializes this weight to one and applies a
        // plain RMSNorm before the SiLU gate. It intentionally differs from
        // the zero-centred AdditiveRmsNorm used by the decoder layers.
        let norm = rms_norm(cfg.linear_value_head_dim, cfg.rms_norm_eps, vb.pp("norm"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN norm", e))?;
        let out_proj = linear_no_bias(value_dim, cfg.hidden_size, vb.pp("out_proj"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load GDN out_proj", e))?;

        Ok(Self {
            in_proj_qkv,
            in_proj_z,
            in_proj_b,
            in_proj_a,
            conv1d,
            decode_conv_weight,
            dt_bias,
            neg_a,
            norm,
            out_proj,
            num_key_heads: cfg.linear_num_key_heads,
            num_value_heads: cfg.linear_num_value_heads,
            key_head_dim: cfg.linear_key_head_dim,
            value_head_dim: cfg.linear_value_head_dim,
            conv_kernel_size: cfg.linear_conv_kernel_dim,
            conv_state: RefCell::new(None),
            recurrent_state: RefCell::new(None),
        })
    }

    fn causal_conv(&self, mixed: &Tensor) -> Result<Tensor, Error> {
        let (batch, channels, seq_len) = mixed
            .dims3()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN convolution input", e))?;
        let previous = self.conv_state.borrow().clone();
        let (output, new_state) = match previous.as_ref() {
            None => {
                let output = self
                    .conv1d
                    .forward(mixed)
                    .and_then(|output| output.narrow(2, 0, seq_len))
                    .map_err(|e| {
                        candle_to_ocr_inference(MODEL_NAME, "GDN causal convolution", e)
                    })?;
                let new_state = if seq_len >= self.conv_kernel_size {
                    mixed.narrow(2, seq_len - self.conv_kernel_size, self.conv_kernel_size)
                } else {
                    let padding = Tensor::zeros(
                        (batch, channels, self.conv_kernel_size - seq_len),
                        mixed.dtype(),
                        mixed.device(),
                    )
                    .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "pad GDN conv state", e))?;
                    Tensor::cat(&[&padding, mixed], 2)
                }
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN update conv state", e))?;
                (output, new_state)
            }
            Some(state) if seq_len == 1 => {
                // A grouped Conv1d launch with one group per channel is very
                // expensive for autoregressive decoding. For a single token,
                // the same depthwise convolution is just a weighted sum of the
                // shifted cache and the new projection.
                cached_depthwise_conv_step(
                    state,
                    mixed,
                    &self.decode_conv_weight,
                    self.conv_kernel_size,
                )
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN decode convolution", e))?
            }
            Some(state) => {
                let joined = Tensor::cat(&[state, mixed], 2).map_err(|e| {
                    candle_to_ocr_inference(MODEL_NAME, "join GDN convolution context", e)
                })?;
                let conv = Conv1d::new(
                    self.conv1d.weight().clone(),
                    None,
                    Conv1dConfig {
                        groups: channels,
                        ..Default::default()
                    },
                );
                let output = conv
                    .forward(&joined)
                    .and_then(|output| output.narrow(2, 1, seq_len))
                    .map_err(|e| {
                        candle_to_ocr_inference(MODEL_NAME, "GDN causal convolution", e)
                    })?;
                let context_len = joined.dim(2).map_err(|e| {
                    candle_to_ocr_inference(MODEL_NAME, "GDN convolution cache length", e)
                })?;
                let new_state = joined
                    .narrow(
                        2,
                        context_len - self.conv_kernel_size,
                        self.conv_kernel_size,
                    )
                    .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN update conv state", e))?;
                (output, new_state)
            }
        };
        *self.conv_state.borrow_mut() = Some(new_state);

        candle_nn::ops::silu(&output)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN convolution SiLU", e))
    }

    fn forward(&self, hidden_states: &Tensor) -> Result<Tensor, Error> {
        let (batch, seq_len, _) = hidden_states
            .dims3()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN input shape", e))?;
        let mixed = self
            .in_proj_qkv
            .forward(hidden_states)
            .and_then(|x| x.transpose(1, 2))
            .and_then(|x| x.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN qkv projection", e))?;
        let mixed = self
            .causal_conv(&mixed)?
            .transpose(1, 2)
            .and_then(|x| x.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN qkv layout", e))?;

        let key_dim = self.num_key_heads * self.key_head_dim;
        let value_dim = self.num_value_heads * self.value_head_dim;
        let query = mixed
            .narrow(D::Minus1, 0, key_dim)
            .and_then(|x| x.reshape((batch, seq_len, self.num_key_heads, self.key_head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN query", e))?;
        let key = mixed
            .narrow(D::Minus1, key_dim, key_dim)
            .and_then(|x| x.reshape((batch, seq_len, self.num_key_heads, self.key_head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN key", e))?;
        let value = mixed
            .narrow(D::Minus1, key_dim * 2, value_dim)
            .and_then(|x| x.reshape((batch, seq_len, self.num_value_heads, self.value_head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN value", e))?;

        let repeat = self.num_value_heads / self.num_key_heads;
        let query = if repeat == 1 {
            query
        } else {
            query
                .unsqueeze(3)
                .and_then(|x| x.repeat((1, 1, 1, repeat, 1)))
                .and_then(|x| x.reshape((batch, seq_len, self.num_value_heads, self.key_head_dim)))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN repeat query heads", e))?
        };
        let key = if repeat == 1 {
            key
        } else {
            key.unsqueeze(3)
                .and_then(|x| x.repeat((1, 1, 1, repeat, 1)))
                .and_then(|x| x.reshape((batch, seq_len, self.num_value_heads, self.key_head_dim)))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN repeat key heads", e))?
        };
        let packed_qkv = Tensor::cat(&[&query, &key, &value], D::Minus1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN pack qkv", e))?;

        let beta = self
            .in_proj_b
            .forward(hidden_states)
            .and_then(|x| candle_nn::ops::sigmoid(&x))
            .and_then(|x| x.to_dtype(DType::F32))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN beta", e))?;
        let a = self
            .in_proj_a
            .forward(hidden_states)
            .and_then(|x| x.to_dtype(DType::F32))
            .and_then(|x| x.broadcast_add(&self.dt_bias))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN decay projection", e))?;
        // Stable equivalent of log(1 + exp(a)), matching torch softplus
        // without overflowing for large positive decay logits.
        let softplus = a
            .relu()
            .and_then(|positive| {
                a.abs()
                    .and_then(|magnitude| magnitude.neg())
                    .and_then(|negative| negative.exp())
                    .and_then(|correction| correction + 1.0)
                    .and_then(|correction| correction.log())
                    .and_then(|correction| positive + correction)
            })
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN softplus", e))?;
        let g = softplus
            .broadcast_mul(&self.neg_a)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN decay", e))?;
        let gb = Tensor::stack(&[&g, &beta], D::Minus1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN pack decay/beta", e))?;

        let initial_state = match self.recurrent_state.borrow().as_ref() {
            Some(state) => state.clone(),
            None => Tensor::zeros(
                (
                    batch,
                    self.num_value_heads,
                    self.key_head_dim,
                    self.value_head_dim,
                ),
                DType::F32,
                hidden_states.device(),
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN initial state", e))?,
        };
        let (core, final_state) = gated_delta_rule(&packed_qkv, &gb, &initial_state)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN recurrence", e))?;
        *self.recurrent_state.borrow_mut() = Some(final_state);

        let z = self
            .in_proj_z
            .forward(hidden_states)
            .and_then(|x| x.reshape((batch, seq_len, self.num_value_heads, self.value_head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN z projection", e))?;
        let core = self
            .norm
            .forward(&core)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN output norm", e))?;
        let gate = z
            .to_dtype(DType::F32)
            .and_then(|z| candle_nn::ops::silu(&z))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN output gate", e))?;
        let core = core
            .to_dtype(DType::F32)
            .and_then(|core| core.broadcast_mul(&gate))
            .and_then(|core| core.to_dtype(hidden_states.dtype()))
            .and_then(|core| core.reshape((batch, seq_len, value_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN gated output", e))?;
        self.out_proj
            .forward(&core)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "GDN output projection", e))
    }

    fn clear_cache(&self) {
        *self.conv_state.borrow_mut() = None;
        *self.recurrent_state.borrow_mut() = None;
    }
}

#[derive(Debug)]
struct FullAttention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    q_norm: AdditiveRmsNorm,
    k_norm: AdditiveRmsNorm,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    scaling: f64,
    kv_cache: RefCell<TrimmableKvCache>,
}

impl FullAttention {
    fn load(cfg: &OvisOcr2TextConfig, vb: VarBuilder) -> Result<Self, Error> {
        if !cfg
            .num_attention_heads
            .is_multiple_of(cfg.num_key_value_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2: num_attention_heads ({}) must be divisible by num_key_value_heads ({})",
                    cfg.num_attention_heads, cfg.num_key_value_heads
                ),
            });
        }
        let q_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_attention_heads * cfg.head_dim * 2,
            vb.pp("q_proj"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load attention q_proj", e))?;
        let k_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_key_value_heads * cfg.head_dim,
            vb.pp("k_proj"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load attention k_proj", e))?;
        let v_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_key_value_heads * cfg.head_dim,
            vb.pp("v_proj"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load attention v_proj", e))?;
        let o_proj = linear_no_bias(
            cfg.num_attention_heads * cfg.head_dim,
            cfg.hidden_size,
            vb.pp("o_proj"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load attention o_proj", e))?;
        let q_norm = AdditiveRmsNorm::load(cfg.head_dim, cfg.rms_norm_eps, vb.pp("q_norm"))?;
        let k_norm = AdditiveRmsNorm::load(cfg.head_dim, cfg.rms_norm_eps, vb.pp("k_norm"))?;
        Ok(Self {
            q_proj,
            k_proj,
            v_proj,
            o_proj,
            q_norm,
            k_norm,
            num_heads: cfg.num_attention_heads,
            num_kv_heads: cfg.num_key_value_heads,
            num_kv_groups: cfg.num_attention_heads / cfg.num_key_value_heads,
            head_dim: cfg.head_dim,
            scaling: 1.0 / (cfg.head_dim as f64).sqrt(),
            kv_cache: RefCell::new(TrimmableKvCache::new(2, cfg.max_position_embeddings)),
        })
    }

    fn apply_rope(&self, tensor: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let rotary_dim = cos
            .dim(D::Minus1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention rotary dimension", e))?;
        let rotary = tensor
            .narrow(D::Minus1, 0, rotary_dim)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention rotary slice", e))?;
        let pass = tensor
            .narrow(D::Minus1, rotary_dim, self.head_dim - rotary_dim)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention pass slice", e))?;
        let cos = cos
            .unsqueeze(1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention cos layout", e))?;
        let sin = sin
            .unsqueeze(1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention sin layout", e))?;
        let rotated = rotate_half(&rotary)?;
        let embedded = (rotary
            .broadcast_mul(&cos)
            .and_then(|lhs| rotated.broadcast_mul(&sin).and_then(|rhs| &lhs + &rhs)))
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "apply attention RoPE", e))?;
        Tensor::cat(&[&embedded, &pass], D::Minus1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention RoPE output", e))
    }

    fn forward(&self, hidden_states: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let (batch, seq_len, _) = hidden_states
            .dims3()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention input", e))?;
        let qg = self
            .q_proj
            .forward(hidden_states)
            .and_then(|x| x.reshape((batch, seq_len, self.num_heads, self.head_dim * 2)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention q/g projection", e))?;
        let q = qg
            .narrow(D::Minus1, 0, self.head_dim)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention q slice", e))?;
        let gate = qg
            .narrow(D::Minus1, self.head_dim, self.head_dim)
            .and_then(|x| x.reshape((batch, seq_len, self.num_heads * self.head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention gate slice", e))?;
        let q = self
            .q_norm
            .forward(&q)?
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention q layout", e))?;
        let k = self
            .k_proj
            .forward(hidden_states)
            .and_then(|x| x.reshape((batch, seq_len, self.num_kv_heads, self.head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention k projection", e))?;
        let k = self
            .k_norm
            .forward(&k)?
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention k layout", e))?;
        let v = self
            .v_proj
            .forward(hidden_states)
            .and_then(|x| x.reshape((batch, seq_len, self.num_kv_heads, self.head_dim)))
            .and_then(|x| x.transpose(1, 2))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention v projection", e))?;
        let q = self
            .apply_rope(&q, cos, sin)?
            .contiguous()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention q contiguous", e))?;
        let k = self
            .apply_rope(&k, cos, sin)?
            .contiguous()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention k contiguous", e))?;
        let v = v
            .contiguous()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention v contiguous", e))?;
        let (k, v) = self
            .kv_cache
            .borrow_mut()
            .append(&k, &v)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention KV cache", e))?;

        let output = match flash_attention(&q, &k, &v, self.scaling, seq_len > 1)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "flash attention", e))?
        {
            Some(output) => output,
            None => scaled_dot_product_attention_gqa(
                &q,
                &k,
                &v,
                None,
                self.scaling,
                true,
                self.num_kv_groups,
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "grouped-query attention", e))?,
        };
        let output = output
            .transpose(1, 2)
            .and_then(|x| x.reshape((batch, seq_len, self.num_heads * self.head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention output layout", e))?;
        let gate = candle_nn::ops::sigmoid(&gate)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention output gate", e))?;
        self.o_proj
            .forward(
                &(&output * &gate).map_err(|e| {
                    candle_to_ocr_inference(MODEL_NAME, "attention gated output", e)
                })?,
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "attention output projection", e))
    }

    fn clear_cache(&self) {
        self.kv_cache.borrow_mut().reset();
    }
}

#[derive(Debug)]
enum TokenMixer {
    Linear(GatedDeltaNet),
    Full(FullAttention),
}

#[derive(Debug)]
struct DecoderLayer {
    mixer: TokenMixer,
    mlp: OvisMlp,
    input_layernorm: AdditiveRmsNorm,
    post_attention_layernorm: AdditiveRmsNorm,
}

impl DecoderLayer {
    fn load(cfg: &OvisOcr2TextConfig, layer_type: &str, vb: VarBuilder) -> Result<Self, Error> {
        let mixer = match layer_type {
            "linear_attention" => {
                TokenMixer::Linear(GatedDeltaNet::load(cfg, vb.pp("linear_attn"))?)
            }
            "full_attention" => TokenMixer::Full(FullAttention::load(cfg, vb.pp("self_attn"))?),
            other => {
                return Err(Error::Config {
                    message: format!("OvisOCR2: unsupported decoder layer type '{other}'"),
                });
            }
        };
        Ok(Self {
            mixer,
            mlp: OvisMlp::load(cfg, vb.pp("mlp"))?,
            input_layernorm: AdditiveRmsNorm::load(
                cfg.hidden_size,
                cfg.rms_norm_eps,
                vb.pp("input_layernorm"),
            )?,
            post_attention_layernorm: AdditiveRmsNorm::load(
                cfg.hidden_size,
                cfg.rms_norm_eps,
                vb.pp("post_attention_layernorm"),
            )?,
        })
    }

    fn forward(&self, hidden_states: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let residual = hidden_states.clone();
        let normalized = self.input_layernorm.forward(hidden_states)?;
        let mixed = match &self.mixer {
            TokenMixer::Linear(layer) => layer.forward(&normalized)?,
            TokenMixer::Full(layer) => layer.forward(&normalized, cos, sin)?,
        };
        let hidden_states = (&residual + &mixed)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "decoder mixer residual", e))?;
        let residual = hidden_states.clone();
        let hidden_states = self.post_attention_layernorm.forward(&hidden_states)?;
        let hidden_states = self.mlp.forward(&hidden_states)?;
        (&residual + &hidden_states)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "decoder MLP residual", e))
    }

    fn clear_cache(&self) {
        match &self.mixer {
            TokenMixer::Linear(layer) => layer.clear_cache(),
            TokenMixer::Full(layer) => layer.clear_cache(),
        }
    }
}

#[derive(Debug, Clone)]
struct TextRotaryEmbedding {
    rotary: RotaryEmbedding,
    axis_ids: Tensor,
}

impl TextRotaryEmbedding {
    fn new(cfg: &OvisOcr2TextConfig, device: &Device) -> Result<Self, Error> {
        let rotary_dim = (cfg.head_dim as f64 * cfg.rope_parameters.partial_rotary_factor) as usize;
        if rotary_dim == 0 || !rotary_dim.is_multiple_of(2) {
            return Err(Error::Config {
                message: format!("OvisOCR2: invalid rotary dimension {rotary_dim}"),
            });
        }
        let half = rotary_dim / 2;
        if cfg.rope_parameters.mrope_section.iter().sum::<usize>() != half {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2: mrope_section {:?} must sum to rotary_dim/2 ({half})",
                    cfg.rope_parameters.mrope_section
                ),
            });
        }
        let rotary =
            RotaryEmbedding::new_multi_axis(rotary_dim, cfg.rope_parameters.rope_theta, 3, device)?;
        let axis_ids = Tensor::from_vec(
            interleaved_axis_ids(rotary_dim, &cfg.rope_parameters.mrope_section),
            (1, 1, rotary_dim, 1),
            device,
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "create mRoPE axis map", e))?;
        Ok(Self { rotary, axis_ids })
    }

    fn forward(&self, position_ids: &Tensor, dtype: DType) -> Result<(Tensor, Tensor), Error> {
        let (cos, sin) = self.rotary.forward_multi_axis(position_ids, dtype)?;
        Ok((self.select_axes(&cos)?, self.select_axes(&sin)?))
    }

    fn select_axes(&self, values: &Tensor) -> Result<Tensor, Error> {
        let (_, batch, seq_len, rotary_dim) = values
            .dims4()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "mRoPE tensor shape", e))?;
        let values = values
            .permute((1, 2, 3, 0))
            .and_then(|values| values.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "mRoPE axis layout", e))?;
        let axis_ids = self
            .axis_ids
            .expand((batch, seq_len, rotary_dim, 1))
            .and_then(|axis_ids| axis_ids.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "expand mRoPE axis map", e))?;
        values
            .gather(&axis_ids, 3)
            .and_then(|values| values.squeeze(3))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "select mRoPE axes", e))
    }
}

fn interleaved_axis_ids(rotary_dim: usize, mrope_section: &[usize]) -> Vec<u32> {
    let half = rotary_dim / 2;
    let h_limit = mrope_section[1] * 3;
    let w_limit = mrope_section[2] * 3;
    (0..rotary_dim)
        .map(|dimension| {
            let freq_idx = dimension % half;
            if freq_idx % 3 == 1 && freq_idx < h_limit {
                1
            } else if freq_idx % 3 == 2 && freq_idx < w_limit {
                2
            } else {
                0
            }
        })
        .collect()
}

pub(crate) struct OvisOcr2TextModel {
    embed_tokens: Embedding,
    layers: Vec<DecoderLayer>,
    norm: AdditiveRmsNorm,
    rotary_emb: TextRotaryEmbedding,
}

impl OvisOcr2TextModel {
    pub(crate) fn load(cfg: &OvisOcr2TextConfig, vb: VarBuilder) -> Result<Self, Error> {
        if cfg.layer_types.len() != cfg.num_hidden_layers {
            return Err(Error::Config {
                message: format!(
                    "OvisOCR2: layer_types has {} entries, expected {}",
                    cfg.layer_types.len(),
                    cfg.num_hidden_layers
                ),
            });
        }
        let embed_tokens = embedding(cfg.vocab_size, cfg.hidden_size, vb.pp("embed_tokens"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load token embeddings", e))?;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for (index, layer_type) in cfg.layer_types.iter().enumerate() {
            layers.push(DecoderLayer::load(
                cfg,
                layer_type,
                vb.pp(format!("layers.{index}")),
            )?);
        }
        let norm = AdditiveRmsNorm::load(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm"))?;
        let rotary_emb = TextRotaryEmbedding::new(cfg, vb.device())?;
        Ok(Self {
            embed_tokens,
            layers,
            norm,
            rotary_emb,
        })
    }

    pub(crate) fn embed(&self, input_ids: &Tensor) -> Result<Tensor, Error> {
        self.embed_tokens
            .forward(input_ids)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "token embedding", e))
    }

    pub(crate) fn token_embedding_weight(&self) -> Tensor {
        self.embed_tokens.embeddings().clone()
    }

    pub(crate) fn forward(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
    ) -> Result<Tensor, Error> {
        let (cos, sin) = self
            .rotary_emb
            .forward(position_ids, inputs_embeds.dtype())?;
        let mut hidden_states = inputs_embeds.clone();
        for layer in &self.layers {
            hidden_states = layer.forward(&hidden_states, &cos, &sin)?;
        }
        self.norm.forward(&hidden_states)
    }

    pub(crate) fn clear_cache(&self) {
        for layer in &self.layers {
            layer.clear_cache();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::IndexOp;

    #[test]
    fn cached_depthwise_step_matches_grouped_convolution() -> candle_core::Result<()> {
        let state = Tensor::from_vec(
            vec![1f32, 2., 3., 4., 5., 6., 7., 8.],
            (1, 2, 4),
            &Device::Cpu,
        )?;
        let mixed = Tensor::from_vec(vec![9f32, 10.], (1, 2, 1), &Device::Cpu)?;
        let weight = Tensor::from_vec(
            vec![0.1f32, 0.2, 0.3, 0.4, -0.5, 0.25, 0.75, 1.0],
            (2, 1, 4),
            &Device::Cpu,
        )?;
        let decode_weight = weight.squeeze(1)?.unsqueeze(0)?;
        let (actual, new_state) = cached_depthwise_conv_step(&state, &mixed, &decode_weight, 4)?;

        let joined = Tensor::cat(&[&state, &mixed], 2)?;
        let conv = Conv1d::new(
            weight,
            None,
            Conv1dConfig {
                groups: 2,
                ..Default::default()
            },
        );
        let expected = conv.forward(&joined)?.narrow(2, 1, 1)?;
        assert_eq!(actual.to_vec3::<f32>()?, expected.to_vec3::<f32>()?);
        assert_eq!(
            new_state.to_vec3::<f32>()?,
            joined.narrow(2, 1, 4)?.to_vec3::<f32>()?
        );
        Ok(())
    }

    #[test]
    fn cached_depthwise_step_matches_low_precision_convolution() -> candle_core::Result<()> {
        for dtype in [DType::BF16, DType::F16] {
            let state = Tensor::from_vec(
                vec![1f32, 2., 3., 4., 5., 6., 7., 8.],
                (1, 2, 4),
                &Device::Cpu,
            )?
            .to_dtype(dtype)?;
            let mixed =
                Tensor::from_vec(vec![9f32, 10.], (1, 2, 1), &Device::Cpu)?.to_dtype(dtype)?;
            let weight = Tensor::from_vec(
                vec![0.1f32, 0.2, 0.3, 0.4, -0.5, 0.25, 0.75, 1.0],
                (2, 1, 4),
                &Device::Cpu,
            )?
            .to_dtype(dtype)?;
            let decode_weight = weight.to_dtype(DType::F32)?.squeeze(1)?.unsqueeze(0)?;
            let (actual, _) = cached_depthwise_conv_step(&state, &mixed, &decode_weight, 4)?;
            let expected = match dtype {
                DType::BF16 => vec![vec![vec![5.59375f32], vec![14.75]]],
                DType::F16 => vec![vec![vec![5.597_656_3f32], vec![14.75]]],
                _ => unreachable!(),
            };
            assert_eq!(actual.dtype(), dtype);
            assert_eq!(
                actual.to_dtype(DType::F32)?.to_vec3::<f32>()?,
                expected,
                "cached convolution mismatch for {dtype:?}"
            );
        }
        Ok(())
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cached_depthwise_step_matches_cuda_low_precision_convolution() -> candle_core::Result<()> {
        let Ok(device) = Device::new_cuda(0) else {
            return Ok(());
        };
        for dtype in [DType::BF16, DType::F16] {
            let state =
                Tensor::from_vec(vec![1f32, 2., 3., 4., 5., 6., 7., 8.], (1, 2, 4), &device)?
                    .to_dtype(dtype)?;
            let mixed = Tensor::from_vec(vec![9f32, 10.], (1, 2, 1), &device)?.to_dtype(dtype)?;
            let weight = Tensor::from_vec(
                vec![0.1f32, 0.2, 0.3, 0.4, -0.5, 0.25, 0.75, 1.0],
                (2, 1, 4),
                &device,
            )?
            .to_dtype(dtype)?;
            let decode_weight = weight.to_dtype(DType::F32)?.squeeze(1)?.unsqueeze(0)?;
            let (actual, _) = cached_depthwise_conv_step(&state, &mixed, &decode_weight, 4)?;

            let joined = Tensor::cat(&[&state, &mixed], 2)?;
            let conv = Conv1d::new(
                weight,
                None,
                Conv1dConfig {
                    groups: 2,
                    ..Default::default()
                },
            );
            let expected = conv.forward(&joined)?.narrow(2, 1, 1)?;
            assert_eq!(actual.dtype(), dtype);
            assert_eq!(
                actual
                    .to_dtype(DType::F32)?
                    .to_device(&Device::Cpu)?
                    .to_vec3::<f32>()?,
                expected
                    .to_dtype(DType::F32)?
                    .to_device(&Device::Cpu)?
                    .to_vec3::<f32>()?,
                "cached convolution mismatch for {dtype:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn qwen35_mrope_axes_are_interleaved() {
        let rotary_dim = 64;
        let sections = [11, 11, 10];
        let rope = TextRotaryEmbedding {
            rotary: RotaryEmbedding::new_multi_axis(rotary_dim, 1.0, 3, &Device::Cpu).unwrap(),
            axis_ids: Tensor::from_vec(
                interleaved_axis_ids(rotary_dim, &sections),
                (1, 1, rotary_dim, 1),
                &Device::Cpu,
            )
            .unwrap(),
        };
        let ids = Tensor::from_vec(
            [vec![10i64; 32], vec![20i64; 32], vec![30i64; 32]].concat(),
            (3, 1, 32),
            &Device::Cpu,
        )
        .unwrap();
        let (cos, _) = rope.forward(&ids, DType::F32).unwrap();
        let values = cos.i((0, 0)).unwrap().to_vec1::<f32>().unwrap();
        assert!((values[0] - 10f32.cos()).abs() < 1e-6);
        assert!((values[1] - 20f32.cos()).abs() < 1e-6);
        assert!((values[2] - 30f32.cos()).abs() < 1e-6);
        assert!((values[31] - 20f32.cos()).abs() < 1e-6);
    }
}
