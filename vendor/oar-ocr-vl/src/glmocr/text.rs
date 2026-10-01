use super::config::GlmOcrTextConfig;
use crate::attention::{flash_attention, scaled_dot_product_attention_gqa};
#[cfg(any(feature = "cuda", test))]
use crate::decoder_graph::decoder_cache_capacity;
#[cfg(feature = "cuda")]
use crate::decoder_graph::{
    CudaGraphDrainGuard, CudaGraphKvLengths, SingleTokenDecoderCudaGraph, cuda_graph_error,
    decoder_attention_is_causal, drop_and_drain, report_stashed_cuda_error, sync_graph_tensor,
};
use crate::error::Error;
#[cfg(feature = "cuda")]
use crate::hunyuanocr::dynamic_kv::DynamicKvAppend;
use crate::kv_trim::TrimmableKvCache;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{D, DType, Device, IndexOp, Tensor};
use candle_nn::{
    Embedding, Linear, Module, RmsNorm, VarBuilder, embedding, linear_no_bias, rms_norm,
};
use std::cell::RefCell;

#[cfg(any(feature = "cuda", test))]
const GLM_DECODE_CACHE_LEN: usize = 16_384;

#[cfg(any(feature = "cuda", test))]
fn ar_cuda_graph_capacity(prompt_len: usize, max_new_tokens: usize) -> Option<usize> {
    decoder_cache_capacity(prompt_len, max_new_tokens, GLM_DECODE_CACHE_LEN)
}

fn rotate_half_interleaved(x: &Tensor) -> Result<Tensor, Error> {
    let (b, h, s, d) = x.dims4().map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half dims4",
            e,
        )
    })?;
    if d % 2 != 0 {
        return Err(Error::Config {
            message: format!("GLM-OCR: head_dim must be even, got {d}"),
        });
    }
    let half = d / 2;
    let x = x.reshape((b, h, s, half, 2)).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half reshape",
            e,
        )
    })?;
    let x_even = x.i((.., .., .., .., 0)).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half even slice",
            e,
        )
    })?;
    let x_odd = x.i((.., .., .., .., 1)).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half odd slice",
            e,
        )
    })?;
    let x_odd = x_odd.neg().map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half neg",
            e,
        )
    })?;
    let stacked = Tensor::stack(&[&x_odd, &x_even], D::Minus1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half stack",
            e,
        )
    })?;
    stacked.flatten_from(D::Minus2).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half flatten",
            e,
        )
    })
}

pub(super) fn apply_rotary_pos_emb(
    q: &Tensor,
    k: &Tensor,
    cos: &Tensor,
    sin: &Tensor,
) -> Result<(Tensor, Tensor), Error> {
    let cos = cos.unsqueeze(1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: cos unsqueeze",
            e,
        )
    })?;
    let sin = sin.unsqueeze(1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: sin unsqueeze",
            e,
        )
    })?;

    let (b, h, s, rot_dim) = cos.dims4().map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: cos dims4",
            e,
        )
    })?;
    let half = rot_dim / 2;
    let cos_half = cos.narrow(D::Minus1, 0, half).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: cos narrow",
            e,
        )
    })?;
    let sin_half = sin.narrow(D::Minus1, 0, half).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: sin narrow",
            e,
        )
    })?;
    let cos = Tensor::stack(&[&cos_half, &cos_half], D::Minus1)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: cos stack",
                e,
            )
        })?
        .reshape((b, h, s, rot_dim))
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: cos reshape",
                e,
            )
        })?;
    let sin = Tensor::stack(&[&sin_half, &sin_half], D::Minus1)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: sin stack",
                e,
            )
        })?
        .reshape((b, h, s, rot_dim))
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: sin reshape",
                e,
            )
        })?;

    let head_dim = q.dim(D::Minus1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: q head_dim",
            e,
        )
    })?;

    let q_rot = q.narrow(D::Minus1, 0, rot_dim).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: q narrow",
            e,
        )
    })?;
    let k_rot = k.narrow(D::Minus1, 0, rot_dim).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: k narrow",
            e,
        )
    })?;

    let q_rotated = rotate_half_interleaved(&q_rot)?;
    let k_rotated = rotate_half_interleaved(&k_rot)?;

    let q_mul = q_rot.broadcast_mul(&cos).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: q*cos",
            e,
        )
    })?;
    let q_rot_mul = q_rotated.broadcast_mul(&sin).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half(q)*sin",
            e,
        )
    })?;
    let mut q_out = (&q_mul + &q_rot_mul).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: q apply rope",
            e,
        )
    })?;

    let k_mul = k_rot.broadcast_mul(&cos).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: k*cos",
            e,
        )
    })?;
    let k_rot_mul = k_rotated.broadcast_mul(&sin).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: rotate_half(k)*sin",
            e,
        )
    })?;
    let mut k_out = (&k_mul + &k_rot_mul).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: k apply rope",
            e,
        )
    })?;

    if rot_dim < head_dim {
        let q_pass = q
            .narrow(D::Minus1, rot_dim, head_dim - rot_dim)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: q pass narrow",
                    e,
                )
            })?;
        let k_pass = k
            .narrow(D::Minus1, rot_dim, head_dim - rot_dim)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: k pass narrow",
                    e,
                )
            })?;
        q_out = Tensor::cat(&[&q_out, &q_pass], D::Minus1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: q cat pass",
                e,
            )
        })?;
        k_out = Tensor::cat(&[&k_out, &k_pass], D::Minus1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: k cat pass",
                e,
            )
        })?;
    }

    Ok((q_out, k_out))
}

fn apply_mrope(freqs: &Tensor, mrope_section: &[usize]) -> Result<Tensor, Error> {
    if mrope_section.is_empty() {
        return Err(Error::Config {
            message: "GLM-OCR: mrope_section is empty".to_string(),
        });
    }
    let dims = freqs.dims();
    if dims.len() != 4 || dims[0] != 3 {
        return Err(Error::InvalidInput {
            message: format!("GLM-OCR: freqs dims mismatch, got {:?}", dims),
        });
    }
    let mut offset = 0usize;
    let mut chunks = Vec::with_capacity(mrope_section.len());
    for (i, &sec) in mrope_section.iter().enumerate() {
        let seg = freqs.narrow(D::Minus1, offset, sec).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: mrope narrow",
                e,
            )
        })?;
        let picked = seg.i((i % 3, .., .., ..)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: mrope pick",
                e,
            )
        })?;
        chunks.push(picked);
        offset += sec;
    }
    let refs: Vec<&Tensor> = chunks.iter().collect();
    Tensor::cat(&refs, D::Minus1).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: mrope cat",
            e,
        )
    })
}

#[derive(Debug, Clone)]
pub(super) struct GlmOcrTextRotaryEmbedding {
    inv_freq: Tensor,
    mrope_section: Vec<usize>,
}

impl GlmOcrTextRotaryEmbedding {
    pub(super) fn new(cfg: &GlmOcrTextConfig, device: &Device) -> Result<Self, Error> {
        if cfg.rope_parameters.rope_type != "default" {
            return Err(Error::Config {
                message: format!(
                    "GLM-OCR: unsupported rope_type '{}'",
                    cfg.rope_parameters.rope_type
                ),
            });
        }
        let head_dim = if cfg.head_dim > 0 {
            cfg.head_dim
        } else {
            cfg.hidden_size / cfg.num_attention_heads
        };
        let dim = (head_dim as f64 * cfg.rope_parameters.partial_rotary_factor).floor() as usize;
        if !dim.is_multiple_of(2) {
            return Err(Error::Config {
                message: format!("GLM-OCR: rotary dim must be even, got {dim}"),
            });
        }
        let rope_theta = cfg.rope_parameters.rope_theta;
        let inv_freq: Vec<f32> = (0..dim)
            .step_by(2)
            .map(|i| (1.0f64 / rope_theta.powf(i as f64 / dim as f64)) as f32)
            .collect();
        let inv_freq = Tensor::from_vec(inv_freq, (dim / 2,), device).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: create inv_freq",
                e,
            )
        })?;

        let section_sum: usize = cfg.rope_parameters.mrope_section.iter().sum();
        if section_sum != dim / 2 {
            return Err(Error::Config {
                message: format!(
                    "GLM-OCR: mrope_section sum ({section_sum}) != dim/2 ({})",
                    dim / 2
                ),
            });
        }

        Ok(Self {
            inv_freq,
            mrope_section: cfg.rope_parameters.mrope_section.clone(),
        })
    }

    pub(super) fn forward(
        &self,
        x: &Tensor,
        position_ids: &Tensor,
    ) -> Result<(Tensor, Tensor), Error> {
        let dtype = x.dtype();
        let dims = position_ids.dims();
        if dims.len() != 3 || dims[0] != 3 {
            return Err(Error::InvalidInput {
                message: format!("GLM-OCR: position_ids must be (3, B, S), got {:?}", dims),
            });
        }

        let position_ids = position_ids.to_dtype(DType::F32).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: position_ids cast",
                e,
            )
        })?;

        let inv_len = self.inv_freq.dims1().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: inv_freq dims1",
                e,
            )
        })?;
        let inv = self.inv_freq.reshape((1, 1, 1, inv_len)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: inv_freq reshape",
                e,
            )
        })?;
        let freqs = position_ids
            .unsqueeze(3)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: position_ids unsqueeze",
                    e,
                )
            })?
            .broadcast_mul(&inv)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: freqs multiply",
                    e,
                )
            })?;

        let freqs = apply_mrope(&freqs, &self.mrope_section)?;
        let emb = Tensor::cat(&[&freqs, &freqs], D::Minus1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: freqs cat",
                e,
            )
        })?;
        let cos = emb.cos().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: cos",
                e,
            )
        })?;
        let sin = emb.sin().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: sin",
                e,
            )
        })?;

        let cos = cos.to_dtype(dtype).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: cos cast",
                e,
            )
        })?;
        let sin = sin.to_dtype(dtype).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: sin cast",
                e,
            )
        })?;
        Ok((cos, sin))
    }
}

#[derive(Debug, Clone)]
struct GlmOcrTextMLP {
    gate_up_proj: Linear,
    down_proj: Linear,
    activation_fn: candle_nn::Activation,
}

impl GlmOcrTextMLP {
    fn load(cfg: &GlmOcrTextConfig, vb: VarBuilder) -> Result<Self, Error> {
        let gate_up_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.intermediate_size * 2,
            vb.pp("mlp").pp("gate_up_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text gate_up_proj", e))?;
        let down_proj = linear_no_bias(
            cfg.intermediate_size,
            cfg.hidden_size,
            vb.pp("mlp").pp("down_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text down_proj", e))?;
        Ok(Self {
            gate_up_proj,
            down_proj,
            activation_fn: cfg.hidden_act,
        })
    }

    fn forward(&self, hidden_states: &Tensor) -> Result<Tensor, Error> {
        let up_states = self
            .gate_up_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text gate_up_proj forward", e))?;
        let mut parts = up_states.chunk(2, D::Minus1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: gate_up_proj chunk",
                e,
            )
        })?;
        if parts.len() != 2 {
            return Err(Error::InvalidInput {
                message: format!(
                    "GLM-OCR: expected 2 chunks from gate_up_proj, got {}",
                    parts.len()
                ),
            });
        }
        let up_states = parts.pop().unwrap();
        let gate = parts.pop().unwrap();
        let gate = gate
            .apply(&self.activation_fn)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text gate act", e))?;
        let up_states = (up_states * gate)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text gate*up", e))?;
        self.down_proj
            .forward(&up_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text down_proj forward", e))
    }
}

#[derive(Debug, Clone)]
struct GlmOcrTextAttention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    scaling: f64,
    kv_cache: RefCell<TrimmableKvCache>,
}

impl GlmOcrTextAttention {
    fn load(cfg: &GlmOcrTextConfig, vb: VarBuilder) -> Result<Self, Error> {
        if !cfg
            .num_attention_heads
            .is_multiple_of(cfg.num_key_value_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "GLM-OCR: num_attention_heads ({}) must be divisible by num_key_value_heads ({})",
                    cfg.num_attention_heads, cfg.num_key_value_heads
                ),
            });
        }

        let q_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_attention_heads * cfg.head_dim,
            vb.pp("self_attn").pp("q_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text q_proj", e))?;
        let k_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_key_value_heads * cfg.head_dim,
            vb.pp("self_attn").pp("k_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text k_proj", e))?;
        let v_proj = linear_no_bias(
            cfg.hidden_size,
            cfg.num_key_value_heads * cfg.head_dim,
            vb.pp("self_attn").pp("v_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text v_proj", e))?;
        let o_proj = linear_no_bias(
            cfg.num_attention_heads * cfg.head_dim,
            cfg.hidden_size,
            vb.pp("self_attn").pp("o_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text o_proj", e))?;

        let cache_cap = cfg.max_position_embeddings.min(16384);
        // Trim/gather-capable KV cache.
        let kv_cache = TrimmableKvCache::new(2, cache_cap);

        Ok(Self {
            q_proj,
            k_proj,
            v_proj,
            o_proj,
            num_heads: cfg.num_attention_heads,
            num_kv_heads: cfg.num_key_value_heads,
            num_kv_groups: cfg.num_attention_heads / cfg.num_key_value_heads,
            head_dim: cfg.head_dim,
            scaling: 1.0 / (cfg.head_dim as f64).sqrt(),
            kv_cache: RefCell::new(kv_cache),
        })
    }

    fn project_qkv(
        &self,
        hidden_states: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
    ) -> Result<(Tensor, Tensor, Tensor), Error> {
        let (b, seq_len, _) = hidden_states.dims3().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: attn hidden dims3",
                e,
            )
        })?;

        let mut q = self
            .q_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text q_proj forward", e))?;
        let mut k = self
            .k_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text k_proj forward", e))?;
        let mut v = self
            .v_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text v_proj forward", e))?;

        q = q
            .reshape((b, seq_len, self.num_heads, self.head_dim))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: q reshape",
                    e,
                )
            })?
            .transpose(1, 2)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: q transpose",
                    e,
                )
            })?
            .contiguous()
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: q contiguous",
                    e,
                )
            })?;
        k = k
            .reshape((b, seq_len, self.num_kv_heads, self.head_dim))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: k reshape",
                    e,
                )
            })?
            .transpose(1, 2)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: k transpose",
                    e,
                )
            })?
            .contiguous()
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: k contiguous",
                    e,
                )
            })?;
        v = v
            .reshape((b, seq_len, self.num_kv_heads, self.head_dim))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: v reshape",
                    e,
                )
            })?
            .transpose(1, 2)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: v transpose",
                    e,
                )
            })?
            .contiguous()
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: v contiguous",
                    e,
                )
            })?;

        (q, k) = apply_rotary_pos_emb(&q, &k, cos, sin)?;
        q = q.contiguous().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: q contiguous post-rope",
                e,
            )
        })?;
        k = k.contiguous().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: k contiguous post-rope",
                e,
            )
        })?;

        Ok((q, k, v))
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor, Error> {
        let (b, seq_len, _) = hidden_states.dims3().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: attn hidden dims3",
                e,
            )
        })?;
        let (q, k, v) = self.project_qkv(hidden_states, cos, sin)?;

        let (k, v) = self.kv_cache.borrow_mut().append(&k, &v).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: kv_cache append",
                e,
            )
        })?;
        let flash = if b == 1 {
            flash_attention(&q, &k, &v, self.scaling, seq_len > 1)
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text flash attention", e))?
        } else {
            None
        };
        let attn = match flash {
            Some(attn) => attn,
            None => scaled_dot_product_attention_gqa(
                &q,
                &k,
                &v,
                attention_mask,
                self.scaling,
                true,
                self.num_kv_groups,
            )
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text grouped-query attention", e))?,
        };

        self.project_attention_output(&attn, b, seq_len)
    }

    fn project_attention_output(
        &self,
        attn: &Tensor,
        batch: usize,
        seq_len: usize,
    ) -> Result<Tensor, Error> {
        let attn = attn
            .transpose(1, 2)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: attn transpose",
                    e,
                )
            })?
            .reshape((batch, seq_len, self.num_heads * self.head_dim))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: attn reshape",
                    e,
                )
            })?;

        self.o_proj
            .forward(&attn)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text o_proj forward", e))
    }

    #[cfg(feature = "cuda")]
    fn prepare_dynamic_cache(&self, query_len: usize, cache_len: usize) -> Result<(), Error> {
        let template = Tensor::zeros(
            (1, self.num_kv_heads, query_len, self.head_dim),
            self.k_proj.weight().dtype(),
            self.k_proj.weight().device(),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic KV template", e))?;
        self.kv_cache
            .borrow_mut()
            .initialize_storage_with_capacity(&template, cache_len)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "initialize dynamic KV", e))
    }

    #[cfg(feature = "cuda")]
    fn forward_dynamic(
        &self,
        hidden_states: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
    ) -> Result<Tensor, Error> {
        let (batch, query_len, _) = hidden_states
            .dims3()
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic attention hidden shape", e))?;
        if batch != 1 {
            return Err(Error::Config {
                message: "GLM-OCR CUDA-graph attention requires batch size 1".to_string(),
            });
        }
        let (q, k, v) = self.project_qkv(hidden_states, cos, sin)?;
        let cache = self.kv_cache.borrow();
        let cache_len = cache.storage_capacity();
        let (cache_k, cache_v) = cache.storage().ok_or_else(|| Error::Config {
            message: "GLM-OCR dynamic KV storage is not initialized".to_string(),
        })?;
        drop(cache);
        let append = DynamicKvAppend {
            query_len,
            cache_len,
        };
        cache_k
            .inplace_op3(&k, kv_lengths, &append)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic key cache append", e))?;
        cache_v
            .inplace_op3(&v, kv_lengths, &append)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic value cache append", e))?;

        let q = q
            .squeeze(0)
            .and_then(|q| q.transpose(0, 1))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic Q layout", e))?;
        let cache_k = cache_k
            .squeeze(0)
            .and_then(|k| k.transpose(0, 1))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic K layout", e))?;
        let cache_v = cache_v
            .squeeze(0)
            .and_then(|v| v.transpose(0, 1))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic V layout", e))?;
        let attn = candle_flash_attn::flash_attn_varlen(
            &q,
            &cache_k,
            &cache_v,
            query_lengths,
            kv_lengths,
            query_len,
            cache_len,
            self.scaling as f32,
            decoder_attention_is_causal(query_len),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic flash attention", e))?
        .transpose(0, 1)
        .and_then(|attn| attn.unsqueeze(0))
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic attention layout", e))?;
        self.project_attention_output(&attn, batch, query_len)
    }

    fn clear_kv_cache(&self) {
        self.kv_cache.borrow_mut().reset();
    }

    #[cfg(feature = "cuda")]
    fn kv_cache_len(&self) -> usize {
        self.kv_cache.borrow().current_seq_len()
    }

    #[cfg(feature = "cuda")]
    fn set_kv_cache_len(&self, len: usize) -> Result<(), Error> {
        self.kv_cache
            .borrow_mut()
            .set_current_len(len)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "set dynamic KV length", e))
    }
}

#[derive(Debug, Clone)]
pub(super) struct GlmOcrTextDecoderLayer {
    self_attn: GlmOcrTextAttention,
    mlp: GlmOcrTextMLP,
    input_layernorm: RmsNorm,
    post_attention_layernorm: RmsNorm,
    post_self_attn_layernorm: RmsNorm,
    post_mlp_layernorm: RmsNorm,
}

impl GlmOcrTextDecoderLayer {
    pub(super) fn load(cfg: &GlmOcrTextConfig, vb: VarBuilder) -> Result<Self, Error> {
        let self_attn = GlmOcrTextAttention::load(cfg, vb.clone())?;
        let mlp = GlmOcrTextMLP::load(cfg, vb.clone())?;
        let input_layernorm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("input_layernorm"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text input_layernorm", e))?;
        let post_attention_layernorm = rms_norm(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_attention_layernorm"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text post_attention_layernorm", e))?;
        let post_self_attn_layernorm = rms_norm(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_self_attn_layernorm"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text post_self_attn_layernorm", e))?;
        let post_mlp_layernorm = rms_norm(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_mlp_layernorm"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text post_mlp_layernorm", e))?;
        Ok(Self {
            self_attn,
            mlp,
            input_layernorm,
            post_attention_layernorm,
            post_self_attn_layernorm,
            post_mlp_layernorm,
        })
    }

    pub(super) fn forward(
        &self,
        hidden_states: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor, Error> {
        let residual = hidden_states.clone();
        let hidden = self
            .input_layernorm
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text input_layernorm forward", e))?;
        let hidden = self.self_attn.forward(&hidden, cos, sin, attention_mask)?;
        let hidden = self
            .post_self_attn_layernorm
            .forward(&hidden)
            .map_err(|e| {
                candle_to_ocr_inference("GLM-OCR", "text post_self_attn_layernorm forward", e)
            })?;
        let hidden = (&residual + &hidden).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: text residual add",
                e,
            )
        })?;

        let residual = hidden.clone();
        let hidden = self
            .post_attention_layernorm
            .forward(&hidden)
            .map_err(|e| {
                candle_to_ocr_inference("GLM-OCR", "text post_attention_layernorm forward", e)
            })?;
        let hidden = self.mlp.forward(&hidden)?;
        let hidden = self.post_mlp_layernorm.forward(&hidden).map_err(|e| {
            candle_to_ocr_inference("GLM-OCR", "text post_mlp_layernorm forward", e)
        })?;
        (&residual + &hidden).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: text residual add mlp",
                e,
            )
        })
    }

    #[cfg(feature = "cuda")]
    pub(super) fn forward_dynamic(
        &self,
        hidden_states: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
    ) -> Result<Tensor, Error> {
        let residual = hidden_states.clone();
        let hidden = self
            .input_layernorm
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text input_layernorm forward", e))?;
        let hidden =
            self.self_attn
                .forward_dynamic(&hidden, cos, sin, query_lengths, kv_lengths)?;
        let hidden = self
            .post_self_attn_layernorm
            .forward(&hidden)
            .map_err(|e| {
                candle_to_ocr_inference("GLM-OCR", "text post_self_attn_layernorm forward", e)
            })?;
        let hidden = (&residual + &hidden).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: text residual add",
                e,
            )
        })?;

        let residual = hidden.clone();
        let hidden = self
            .post_attention_layernorm
            .forward(&hidden)
            .map_err(|e| {
                candle_to_ocr_inference("GLM-OCR", "text post_attention_layernorm forward", e)
            })?;
        let hidden = self.mlp.forward(&hidden)?;
        let hidden = self.post_mlp_layernorm.forward(&hidden).map_err(|e| {
            candle_to_ocr_inference("GLM-OCR", "text post_mlp_layernorm forward", e)
        })?;
        (&residual + &hidden).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: text residual add mlp",
                e,
            )
        })
    }

    pub(super) fn clear_kv_cache(&self) {
        self.self_attn.clear_kv_cache();
    }

    #[cfg(feature = "cuda")]
    pub(super) fn trim_kv_cache(&self, len: usize) -> Result<(), Error> {
        self.self_attn
            .kv_cache
            .borrow_mut()
            .trim_to(len)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "trim dynamic KV cache", e))
    }

    #[cfg(feature = "cuda")]
    pub(super) fn prepare_dynamic_cache(
        &self,
        query_len: usize,
        cache_len: usize,
    ) -> Result<(), Error> {
        self.self_attn.prepare_dynamic_cache(query_len, cache_len)
    }

    #[cfg(feature = "cuda")]
    pub(super) fn kv_cache_len(&self) -> usize {
        self.self_attn.kv_cache_len()
    }

    #[cfg(feature = "cuda")]
    pub(super) fn set_kv_cache_len(&self, len: usize) -> Result<(), Error> {
        self.self_attn.set_kv_cache_len(len)
    }
}

#[cfg(feature = "cuda")]
struct GlmVerificationCudaGraph {
    // The graph owns device pointers into all tensors below; dispose via
    // `dispose` so capture-touched buffers are never returned to the
    // stream-ordered allocator (see SingleTokenDecoderCudaGraph::dispose).
    graph: candle_core::cuda_backend::cudarc::driver::CudaGraph,
    hidden_input: Tensor,
    position_input: Tensor,
    _query_lengths: Tensor,
    kv_lengths: CudaGraphKvLengths,
    hidden_output: Tensor,
    token_output: Tensor,
    cache_len: usize,
    query_len: usize,
}

#[cfg(feature = "cuda")]
impl GlmVerificationCudaGraph {
    fn dispose(self) {
        let Self {
            graph,
            hidden_input,
            position_input,
            _query_lengths,
            kv_lengths,
            hidden_output,
            token_output,
            cache_len: _,
            query_len: _,
        } = self;
        let device = hidden_input.device().clone();
        report_stashed_cuda_error(&device, "CUDA graph disposal");
        drop_and_drain(graph, &device);
        drop_and_drain(token_output, &device);
        drop_and_drain(hidden_output, &device);
        drop_and_drain(kv_lengths, &device);
        drop_and_drain(_query_lengths, &device);
        drop_and_drain(position_input, &device);
        drop_and_drain(hidden_input, &device);
    }
}

#[cfg(feature = "cuda")]
impl std::fmt::Debug for GlmVerificationCudaGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlmVerificationCudaGraph")
            .field("query_len", &self.query_len)
            .field("cache_len", &self.cache_len)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct GlmOcrTextModel {
    #[cfg(feature = "cuda")]
    decode_graph: RefCell<Option<SingleTokenDecoderCudaGraph>>,
    #[cfg(feature = "cuda")]
    verification_graph: RefCell<Option<GlmVerificationCudaGraph>>,
    embed_tokens: Embedding,
    layers: Vec<GlmOcrTextDecoderLayer>,
    norm: RmsNorm,
    rotary_emb: GlmOcrTextRotaryEmbedding,
    // Must stay the last field: it drops last and drains CUDA errors the
    // other fields' frees may stash (see CudaGraphDrainGuard).
    #[cfg(feature = "cuda")]
    _drain_guard: CudaGraphDrainGuard,
}

impl GlmOcrTextModel {
    pub fn load(cfg: &GlmOcrTextConfig, vb: VarBuilder) -> Result<Self, Error> {
        let embed_tokens = embedding(cfg.vocab_size, cfg.hidden_size, vb.pp("embed_tokens"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text embed_tokens", e))?;
        let rotary_emb = GlmOcrTextRotaryEmbedding::new(cfg, vb.device())?;

        let vb_layers = vb.pp("layers");
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for idx in 0..cfg.num_hidden_layers {
            layers.push(GlmOcrTextDecoderLayer::load(cfg, vb_layers.pp(idx))?);
        }
        let norm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text norm", e))?;

        #[cfg(feature = "cuda")]
        let _drain_guard = CudaGraphDrainGuard::new(vb.device());
        Ok(Self {
            #[cfg(feature = "cuda")]
            decode_graph: RefCell::new(None),
            #[cfg(feature = "cuda")]
            verification_graph: RefCell::new(None),
            embed_tokens,
            layers,
            norm,
            rotary_emb,
            #[cfg(feature = "cuda")]
            _drain_guard,
        })
    }

    pub fn embed(&self, input_ids: &Tensor) -> Result<Tensor, Error> {
        self.embed_tokens
            .forward(input_ids)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text embed forward", e))
    }

    pub fn token_embedding_weight(&self) -> Tensor {
        self.embed_tokens.embeddings().clone()
    }

    pub fn forward(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor, Error> {
        let (cos, sin) = self.rotary_emb.forward(inputs_embeds, position_ids)?;
        self.forward_prepared(inputs_embeds, &cos, &sin, attention_mask)
    }

    fn forward_prepared(
        &self,
        inputs_embeds: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor, Error> {
        let mut hidden_states = inputs_embeds.clone();
        for layer in &self.layers {
            hidden_states = layer.forward(&hidden_states, cos, sin, attention_mask)?;
        }
        self.norm
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "text norm forward", e))
    }

    fn project_logits(&self, hidden_states: &Tensor, lm_head: &Linear) -> Result<Tensor, Error> {
        lm_head
            .forward(hidden_states)
            .and_then(|logits| logits.i((0, 0, ..)))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "decode LM head", e))
    }

    #[cfg(feature = "cuda")]
    fn project_all_logits(
        &self,
        hidden_states: &Tensor,
        lm_head: &Linear,
    ) -> Result<Tensor, Error> {
        lm_head
            .forward(hidden_states)
            .and_then(|logits| logits.squeeze(0))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification LM head", e))
    }

    pub(crate) fn forward_decode_logits(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
        lm_head: &Linear,
    ) -> Result<Tensor, Error> {
        #[cfg(feature = "cuda")]
        {
            let kv_len = self.kv_cache_len().saturating_add(1);
            if let Some(logits) = self.replay_cuda_graph(inputs_embeds, position_ids, kv_len)? {
                return Ok(logits);
            }
        }
        let hidden = self.forward(inputs_embeds, position_ids, None)?;
        self.project_logits(&hidden, lm_head)
    }

    /// Verify a fixed block of speculative tokens in one causal target pass.
    ///
    /// Returned hidden states are the target model's post-final-norm states;
    /// GLM-OCR's MTP layer consumes exactly these states on its next sync pass.
    #[cfg(feature = "cuda")]
    pub(crate) fn forward_verification_tokens(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
        lm_head: &Linear,
    ) -> Result<(Tensor, Tensor), Error> {
        let query_len = inputs_embeds
            .dim(1)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification query length", e))?;
        let kv_len = self.kv_cache_len().saturating_add(query_len);
        if let Some(output) =
            self.replay_verification_cuda_graph(inputs_embeds, position_ids, kv_len)?
        {
            return Ok(output);
        }

        let hidden = self.forward(inputs_embeds, position_ids, None)?;
        let tokens = self
            .project_all_logits(&hidden, lm_head)?
            .argmax(D::Minus1)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification argmax", e))?;
        Ok((hidden, tokens))
    }

    #[cfg(feature = "cuda")]
    fn forward_dynamic(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
    ) -> Result<Tensor, Error> {
        let (cos, sin) = self.rotary_emb.forward(inputs_embeds, position_ids)?;
        let mut hidden_states = inputs_embeds.clone();
        for layer in &self.layers {
            hidden_states =
                layer.forward_dynamic(&hidden_states, &cos, &sin, query_lengths, kv_lengths)?;
        }
        self.norm
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "dynamic text norm", e))
    }

    pub(crate) fn prepare_ar_cuda_graph(
        &self,
        prompt_len: usize,
        max_new_tokens: usize,
        lm_head: &Linear,
    ) -> Result<(), Error> {
        if std::env::var_os("OAR_VL_DISABLE_CUDA_GRAPH").is_some()
            || std::env::var_os("OAR_GLMOCR_DISABLE_CUDA_GRAPH").is_some()
        {
            #[cfg(feature = "cuda")]
            self.invalidate_cuda_graph();
            return Ok(());
        }
        #[cfg(feature = "cuda")]
        if self.embed_tokens.embeddings().device().is_cuda()
            && matches!(
                self.embed_tokens.embeddings().dtype(),
                DType::BF16 | DType::F16
            )
        {
            let Some(cache_len) = ar_cuda_graph_capacity(prompt_len, max_new_tokens) else {
                self.invalidate_cuda_graph();
                return Ok(());
            };
            let required = prompt_len
                .saturating_add(max_new_tokens)
                .min(GLM_DECODE_CACHE_LEN);
            let reusable = self
                .decode_graph
                .borrow()
                .as_ref()
                .is_some_and(|graph| graph.cache_len >= required);
            if reusable {
                return Ok(());
            }
            self.invalidate_cuda_graph();
            self.capture_cuda_graph(cache_len, lm_head)?;
        }
        let _ = prompt_len;
        let _ = max_new_tokens;
        let _ = lm_head;
        Ok(())
    }

    /// Capture the target-model verification block used by greedy MTP.
    #[cfg(feature = "cuda")]
    pub(crate) fn prepare_verification_cuda_graph(
        &self,
        prompt_len: usize,
        max_new_tokens: usize,
        query_len: usize,
        lm_head: &Linear,
    ) -> Result<Option<usize>, Error> {
        if query_len == 0
            || std::env::var_os("OAR_VL_DISABLE_CUDA_GRAPH").is_some()
            || std::env::var_os("OAR_GLMOCR_DISABLE_CUDA_GRAPH").is_some()
        {
            self.invalidate_cuda_graph();
            return Ok(None);
        }
        if !self.embed_tokens.embeddings().device().is_cuda()
            || !matches!(
                self.embed_tokens.embeddings().dtype(),
                DType::BF16 | DType::F16
            )
        {
            self.invalidate_cuda_graph();
            return Ok(None);
        }

        // Verification can temporarily place a complete query block in KV
        // beyond the user-visible output limit, so reserve one extra block.
        let Some(cache_len) =
            ar_cuda_graph_capacity(prompt_len, max_new_tokens.saturating_add(query_len))
        else {
            self.invalidate_cuda_graph();
            return Ok(None);
        };
        let required = prompt_len
            .saturating_add(max_new_tokens)
            .saturating_add(query_len)
            .min(GLM_DECODE_CACHE_LEN);
        let retained_cache_len = self
            .verification_graph
            .borrow()
            .as_ref()
            .filter(|graph| graph.query_len == query_len && graph.cache_len >= required)
            .map(|graph| graph.cache_len);
        if let Some(retained_cache_len) = retained_cache_len {
            // The MTP graph shares this capacity contract. Returning the newly
            // computed (possibly smaller) bucket would force it to recapture
            // even though the retained target graph is still reusable.
            return Ok(Some(retained_cache_len));
        }

        self.invalidate_cuda_graph();
        self.capture_verification_cuda_graph(cache_len, query_len, lm_head)?;
        Ok(Some(cache_len))
    }

    #[cfg(feature = "cuda")]
    fn capture_cuda_graph(&self, cache_len: usize, lm_head: &Linear) -> Result<(), Error> {
        use candle_core::cuda_backend::cudarc::driver::sys::{
            CUgraphInstantiate_flags_enum, CUstreamCaptureMode_enum,
        };

        if self.decode_graph.borrow().is_some() {
            return Ok(());
        }
        let Device::Cuda(cuda) = self.embed_tokens.embeddings().device() else {
            return Ok(());
        };
        let query_len = 1;
        for layer in &self.layers {
            layer
                .self_attn
                .prepare_dynamic_cache(query_len, cache_len)?;
        }
        let hidden_size = self
            .embed_tokens
            .embeddings()
            .dim(1)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "graph hidden size", e))?;
        let device = self.embed_tokens.embeddings().device();
        let hidden_input = Tensor::zeros(
            (1, query_len, hidden_size),
            self.embed_tokens.embeddings().dtype(),
            device,
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "graph hidden input", e))?;
        let position_input = Tensor::zeros((3, 1, query_len), DType::I64, device)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "graph position input", e))?;
        let query_lengths = Tensor::new(&[0u32, query_len as u32], device)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "graph query lengths", e))?;
        let kv_lengths = CudaGraphKvLengths::new(query_len, device)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "graph KV lengths", e))?;
        let stream = cuda.cuda_stream();
        let _htod_cache = cuda.enable_cuda_graph_htod_cache();

        let warm = self.forward_dynamic(
            &hidden_input,
            &position_input,
            &query_lengths,
            kv_lengths.tensor(),
        )?;
        let warm_logits = self.project_logits(&warm, lm_head)?;
        sync_graph_tensor("GLM-OCR", &warm_logits, "warm decoder CUDA graph")?;
        // Allocate the output buffer before capture so it belongs to the
        // regular stream-ordered pool; a capture-time allocation lives in the
        // graph's private pool and can never be returned to the allocator
        // safely. Prime the copy so the captured run sees a warm kernel.
        let logits_output = Tensor::zeros_like(&warm_logits)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "graph logits output", e))?;
        logits_output
            .slice_set(&warm_logits, 0, 0)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "prime graph logits copy", e))?;

        stream
            .begin_capture(CUstreamCaptureMode_enum::CU_STREAM_CAPTURE_MODE_GLOBAL)
            .map_err(|e| cuda_graph_error("GLM-OCR", "begin decoder CUDA graph capture", e))?;
        let captured_output: Result<(), Error> = (|| {
            let hidden = self.forward_dynamic(
                &hidden_input,
                &position_input,
                &query_lengths,
                kv_lengths.tensor(),
            )?;
            let logits = self.project_logits(&hidden, lm_head)?;
            logits_output
                .slice_set(&logits, 0, 0)
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "record graph logits copy", e))
        })();
        if let Err(error) = captured_output {
            let _ = stream.end_capture(
                CUgraphInstantiate_flags_enum::CUDA_GRAPH_INSTANTIATE_FLAG_AUTO_FREE_ON_LAUNCH,
            );
            return Err(error);
        }
        let graph = stream
            .end_capture(
                CUgraphInstantiate_flags_enum::CUDA_GRAPH_INSTANTIATE_FLAG_AUTO_FREE_ON_LAUNCH,
            )
            .map_err(|e| cuda_graph_error("GLM-OCR", "end decoder CUDA graph capture", e))?
            .ok_or_else(|| Error::Config {
                message: "GLM-OCR decoder capture returned no graph".to_string(),
            })?;
        graph
            .launch()
            .map_err(|e| cuda_graph_error("GLM-OCR", "warm decoder CUDA graph", e))?;
        sync_graph_tensor("GLM-OCR", &logits_output, "sync decoder CUDA graph")?;
        self.clear_kv_cache();
        *self.decode_graph.borrow_mut() = Some(SingleTokenDecoderCudaGraph {
            graph,
            hidden_input,
            position_input,
            _query_lengths: query_lengths,
            kv_lengths,
            logits_output,
            cache_len,
        });
        Ok(())
    }

    #[cfg(feature = "cuda")]
    fn capture_verification_cuda_graph(
        &self,
        cache_len: usize,
        query_len: usize,
        lm_head: &Linear,
    ) -> Result<(), Error> {
        use candle_core::cuda_backend::cudarc::driver::sys::{
            CUgraphInstantiate_flags_enum, CUstreamCaptureMode_enum,
        };

        let Device::Cuda(cuda) = self.embed_tokens.embeddings().device() else {
            return Ok(());
        };
        for layer in &self.layers {
            layer.prepare_dynamic_cache(query_len, cache_len)?;
        }
        let hidden_size = self
            .embed_tokens
            .embeddings()
            .dim(1)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "graph hidden size", e))?;
        let device = self.embed_tokens.embeddings().device();
        let hidden_input = Tensor::zeros(
            (1, query_len, hidden_size),
            self.embed_tokens.embeddings().dtype(),
            device,
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification graph input", e))?;
        let position_input = Tensor::zeros((3, 1, query_len), DType::I64, device)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification graph positions", e))?;
        let query_lengths = Tensor::new(&[0u32, query_len as u32], device)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification query lengths", e))?;
        let kv_lengths = CudaGraphKvLengths::new(query_len, device)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification KV lengths", e))?;
        let stream = cuda.cuda_stream();
        let _htod_cache = cuda.enable_cuda_graph_htod_cache();

        let warm = self.forward_dynamic(
            &hidden_input,
            &position_input,
            &query_lengths,
            kv_lengths.tensor(),
        )?;
        let warm_tokens = self
            .project_all_logits(&warm, lm_head)?
            .argmax(D::Minus1)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "warm verification argmax", e))?;
        sync_graph_tensor("GLM-OCR", &warm_tokens, "warm verification CUDA graph")?;
        // Allocate the output buffers before capture so they belong to the
        // regular stream-ordered pool; a capture-time allocation lives in the
        // graph's private pool and can never be returned to the allocator
        // safely. Prime the copies so the captured run sees warm kernels.
        let hidden_output = Tensor::zeros_like(&warm)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification hidden output", e))?;
        let token_output = Tensor::zeros_like(&warm_tokens)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "verification token output", e))?;
        hidden_output
            .slice_set(&warm, 0, 0)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "prime verification hidden copy", e))?;
        token_output
            .slice_set(&warm_tokens, 0, 0)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "prime verification token copy", e))?;

        stream
            .begin_capture(CUstreamCaptureMode_enum::CU_STREAM_CAPTURE_MODE_GLOBAL)
            .map_err(|e| cuda_graph_error("GLM-OCR", "begin verification graph capture", e))?;
        let captured_output: Result<(), Error> = (|| {
            let hidden = self.forward_dynamic(
                &hidden_input,
                &position_input,
                &query_lengths,
                kv_lengths.tensor(),
            )?;
            let tokens = self
                .project_all_logits(&hidden, lm_head)?
                .argmax(D::Minus1)
                .map_err(|e| {
                    candle_to_ocr_inference("GLM-OCR", "captured verification argmax", e)
                })?;
            hidden_output.slice_set(&hidden, 0, 0).map_err(|e| {
                candle_to_ocr_inference("GLM-OCR", "record verification hidden copy", e)
            })?;
            token_output.slice_set(&tokens, 0, 0).map_err(|e| {
                candle_to_ocr_inference("GLM-OCR", "record verification token copy", e)
            })
        })();
        if let Err(error) = captured_output {
            let _ = stream.end_capture(
                CUgraphInstantiate_flags_enum::CUDA_GRAPH_INSTANTIATE_FLAG_AUTO_FREE_ON_LAUNCH,
            );
            return Err(error);
        }
        let graph = stream
            .end_capture(
                CUgraphInstantiate_flags_enum::CUDA_GRAPH_INSTANTIATE_FLAG_AUTO_FREE_ON_LAUNCH,
            )
            .map_err(|e| cuda_graph_error("GLM-OCR", "end verification graph capture", e))?
            .ok_or_else(|| Error::Config {
                message: "GLM-OCR verification capture returned no graph".to_string(),
            })?;
        graph
            .launch()
            .map_err(|e| cuda_graph_error("GLM-OCR", "warm verification CUDA graph", e))?;
        sync_graph_tensor("GLM-OCR", &token_output, "sync verification CUDA graph")?;
        self.clear_kv_cache();
        *self.verification_graph.borrow_mut() = Some(GlmVerificationCudaGraph {
            graph,
            hidden_input,
            position_input,
            _query_lengths: query_lengths,
            kv_lengths,
            hidden_output,
            token_output,
            cache_len,
            query_len,
        });
        Ok(())
    }

    #[cfg(feature = "cuda")]
    fn replay_cuda_graph(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
        kv_len: usize,
    ) -> Result<Option<Tensor>, Error> {
        let captured_ref = self.decode_graph.borrow();
        let Some(captured) = captured_ref.as_ref() else {
            return Ok(None);
        };
        if kv_len > captured.cache_len {
            drop(captured_ref);
            self.invalidate_cuda_graph();
            return Ok(None);
        }
        if inputs_embeds.shape() != captured.hidden_input.shape()
            || position_ids.shape() != captured.position_input.shape()
        {
            return Ok(None);
        }
        captured
            .hidden_input
            .slice_set(inputs_embeds, 0, 0)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "copy graph hidden", e))?;
        captured
            .position_input
            .slice_set(position_ids, 0, 0)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "copy graph positions", e))?;
        captured
            .kv_lengths
            .update(kv_len)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "update graph KV lengths", e))?;
        captured
            .graph
            .launch()
            .map_err(|e| cuda_graph_error("GLM-OCR", "launch decoder CUDA graph", e))?;
        for layer in &self.layers {
            layer.set_kv_cache_len(kv_len)?;
        }
        Ok(Some(captured.logits_output.clone()))
    }

    #[cfg(feature = "cuda")]
    fn replay_verification_cuda_graph(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
        kv_len: usize,
    ) -> Result<Option<(Tensor, Tensor)>, Error> {
        let captured_ref = self.verification_graph.borrow();
        let Some(captured) = captured_ref.as_ref() else {
            return Ok(None);
        };
        if kv_len > captured.cache_len {
            drop(captured_ref);
            self.invalidate_cuda_graph();
            return Ok(None);
        }
        if inputs_embeds.shape() != captured.hidden_input.shape()
            || position_ids.shape() != captured.position_input.shape()
        {
            return Ok(None);
        }
        captured
            .hidden_input
            .slice_set(inputs_embeds, 0, 0)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "copy verification hidden", e))?;
        captured
            .position_input
            .slice_set(position_ids, 0, 0)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "copy verification positions", e))?;
        captured
            .kv_lengths
            .update(kv_len)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "update verification KV lengths", e))?;
        captured
            .graph
            .launch()
            .map_err(|e| cuda_graph_error("GLM-OCR", "launch verification CUDA graph", e))?;
        for layer in &self.layers {
            layer.set_kv_cache_len(kv_len)?;
        }
        Ok(Some((
            captured.hidden_output.clone(),
            captured.token_output.clone(),
        )))
    }

    #[cfg(feature = "cuda")]
    fn invalidate_cuda_graph(&self) {
        if let Some(graph) = self.decode_graph.borrow_mut().take() {
            graph.dispose();
        }
        if let Some(graph) = self.verification_graph.borrow_mut().take() {
            graph.dispose();
        }
    }

    #[cfg(feature = "cuda")]
    fn kv_cache_len(&self) -> usize {
        let len = self.layers.first().map_or(0, |layer| layer.kv_cache_len());
        debug_assert!(self.layers.iter().all(|layer| layer.kv_cache_len() == len));
        len
    }

    pub fn clear_kv_cache(&self) {
        for layer in &self.layers {
            layer.clear_kv_cache();
        }
    }

    #[cfg(feature = "cuda")]
    pub(crate) fn trim_kv_cache(&self, len: usize) -> Result<(), Error> {
        for layer in &self.layers {
            layer.trim_kv_cache(len)?;
        }
        Ok(())
    }
}

#[cfg(feature = "cuda")]
impl Drop for GlmOcrTextModel {
    fn drop(&mut self) {
        // A cached graph must go through dispose: plainly dropping it returns
        // graph-bound buffers to the allocator and poisons it.
        self.invalidate_cuda_graph();
    }
}

#[cfg(test)]
mod tests {
    use super::{GLM_DECODE_CACHE_LEN, ar_cuda_graph_capacity};

    #[test]
    fn ar_graph_capacity_uses_bounded_power_of_two_buckets() {
        assert_eq!(ar_cuda_graph_capacity(1500, 256), Some(2048));
        assert_eq!(ar_cuda_graph_capacity(2000, 4096), Some(8192));
        assert_eq!(
            ar_cuda_graph_capacity(10_000, 20_000),
            Some(GLM_DECODE_CACHE_LEN)
        );
        assert_eq!(ar_cuda_graph_capacity(100, 0), None);
        assert_eq!(ar_cuda_graph_capacity(GLM_DECODE_CACHE_LEN, 1), None);
    }
}
