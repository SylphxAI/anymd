use super::config::HunyuanOcrConfig;
#[cfg(feature = "cuda")]
use super::dynamic_kv::{
    DynamicKvAppend, FusedAddRmsNormBf16, FusedSiluMulBf16, FusedXdRope, FusedXdRopeRmsNormF16,
};
use crate::attention::{
    RotaryEmbedding, flash_attention, scaled_dot_product_attention_gqa, select_rope_sections,
};
#[cfg(feature = "cuda")]
use crate::decoder_graph::{
    CudaGraphDrainGuard, CudaGraphKvLengths, cuda_graph_error, decoder_attention_is_causal,
    drop_and_drain, report_stashed_cuda_error, sync_graph_tensor,
};
use crate::error::Error;
use crate::kv_trim::TrimmableKvCache;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing, rotate_half};
#[cfg(feature = "cuda")]
use candle_core::Device;
use candle_core::{D, Tensor};
use candle_nn::Module;
use std::cell::RefCell;

const DECODE_ROPE_CACHE_LEN: usize = 16_384;

#[cfg(any(feature = "cuda", test))]
fn target_cuda_graph_dtype_supported(dtype: candle_core::DType) -> bool {
    matches!(dtype, candle_core::DType::F16 | candle_core::DType::BF16)
}

#[cfg(any(feature = "cuda", test))]
fn ar_cuda_graph_capacity(prompt_len: usize, max_new_tokens: usize) -> Option<usize> {
    if max_new_tokens == 0 || prompt_len >= DECODE_ROPE_CACHE_LEN {
        return None;
    }
    let required = prompt_len
        .saturating_add(max_new_tokens)
        .min(DECODE_ROPE_CACHE_LEN);
    Some(
        required
            .max(1)
            .next_power_of_two()
            .min(DECODE_ROPE_CACHE_LEN),
    )
}

/// Apply XDRoPE to `(q, k)` using already-section-mixed F32 `cos`/`sin`.
///
/// The section-mix (`select_rope_sections`) and the F32 cast of cos/sin are
/// layer-invariant — only q/k change between layers — so the caller
/// ([`HunyuanLlm::forward`]) hoists those steps out of the layer loop and
/// hands us the prepared tensors. Each layer then only pays the q/k F32 cast
/// and the actual rotary multiply.
fn apply_xdrope_rotary_pos_emb(
    q: &Tensor,
    k: &Tensor,
    cos: &Tensor,
    sin: &Tensor,
) -> Result<(Tensor, Tensor), Error> {
    // Match upstream HF (`apply_rotary_pos_emb_xdrope`): apply the rotary
    // mix in F32, then cast q/k back to the original dtype.
    use candle_core::DType;
    let origin_dtype = q.dtype();
    let q_f32 = q
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "xdrope q to_dtype f32", e))?;
    let k_f32 = k
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "xdrope k to_dtype f32", e))?;

    let q_mul = q_f32.broadcast_mul(cos).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "HunyuanOCR: xdrope q*cos failed",
            e,
        )
    })?;
    let q_half = rotate_half(&q_f32)?;
    let q_half_mul = q_half.broadcast_mul(sin).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "HunyuanOCR: xdrope rotate_half(q)*sin failed",
            e,
        )
    })?;
    let q_rot = (&q_mul + &q_half_mul)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: xdrope apply on q failed",
                e,
            )
        })?
        .to_dtype(origin_dtype)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "xdrope q to_dtype back", e))?;

    let k_mul = k_f32.broadcast_mul(cos).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "HunyuanOCR: xdrope k*cos failed",
            e,
        )
    })?;
    let k_half = rotate_half(&k_f32)?;
    let k_half_mul = k_half.broadcast_mul(sin).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "HunyuanOCR: xdrope rotate_half(k)*sin failed",
            e,
        )
    })?;
    let k_rot = (&k_mul + &k_half_mul)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: xdrope apply on k failed",
                e,
            )
        })?
        .to_dtype(origin_dtype)
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "xdrope k to_dtype back", e))?;
    Ok((q_rot, k_rot))
}

#[derive(Debug)]
struct HunyuanMlp {
    gate_up_proj: candle_nn::Linear,
    down_proj: candle_nn::Linear,
    intermediate_size: usize,
}

impl HunyuanMlp {
    fn load(cfg: &HunyuanOcrConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let gate_proj =
            candle_nn::linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("gate_proj"))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load gate_proj", e))?;
        let up_proj =
            candle_nn::linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("up_proj"))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load up_proj", e))?;
        let down_proj =
            candle_nn::linear_no_bias(cfg.intermediate_size, cfg.hidden_size, vb.pp("down_proj"))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load down_proj", e))?;
        let gate_up_weight = Tensor::cat(&[gate_proj.weight(), up_proj.weight()], 0)
            .and_then(|weight| weight.contiguous())
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "fuse gate/up weights", e))?;
        Ok(Self {
            gate_up_proj: candle_nn::Linear::new(gate_up_weight, None),
            down_proj,
            intermediate_size: cfg.intermediate_size,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor, Error> {
        let gate_up = self
            .gate_up_proj
            .forward(xs)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mlp gate_up_proj", e))?;
        let gate = gate_up
            .narrow(D::Minus1, 0, self.intermediate_size)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mlp gate", e))?;
        let up = gate_up
            .narrow(D::Minus1, self.intermediate_size, self.intermediate_size)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mlp up", e))?;
        #[cfg(feature = "cuda")]
        let prod = if gate.device().is_cuda() && gate.dtype() == candle_core::DType::BF16 {
            gate.apply_op2_no_bwd(&up, &FusedSiluMulBf16)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "fused mlp silu*up", e))?
        } else {
            let gate = candle_nn::ops::silu(&gate)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mlp silu", e))?;
            (&gate * &up).map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mlp gate*up", e))?
        };
        #[cfg(not(feature = "cuda"))]
        let prod = {
            let gate = candle_nn::ops::silu(&gate)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mlp silu", e))?;
            (&gate * &up).map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mlp gate*up", e))?
        };
        self.down_proj
            .forward(&prod)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "mlp down_proj", e))
    }
}

#[derive(Debug)]
struct HunyuanAttention {
    qkv_proj: candle_nn::Linear,
    o_proj: candle_nn::Linear,
    query_layernorm: Option<candle_nn::RmsNorm>,
    key_layernorm: Option<candle_nn::RmsNorm>,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    scaling: f64,
    kv_cache: RefCell<TrimmableKvCache>,
}

impl HunyuanAttention {
    fn load(cfg: &HunyuanOcrConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        if !cfg
            .num_attention_heads
            .is_multiple_of(cfg.num_key_value_heads)
        {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: num_attention_heads ({}) must be divisible by num_key_value_heads ({})",
                    cfg.num_attention_heads, cfg.num_key_value_heads
                ),
            });
        }

        let q_proj = candle_nn::linear_no_bias(
            cfg.hidden_size,
            cfg.num_attention_heads * cfg.head_dim,
            vb.pp("q_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load q_proj", e))?;
        let k_proj = candle_nn::linear_no_bias(
            cfg.hidden_size,
            cfg.num_key_value_heads * cfg.head_dim,
            vb.pp("k_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load k_proj", e))?;
        let v_proj = candle_nn::linear_no_bias(
            cfg.hidden_size,
            cfg.num_key_value_heads * cfg.head_dim,
            vb.pp("v_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load v_proj", e))?;
        let qkv_weight = Tensor::cat(&[q_proj.weight(), k_proj.weight(), v_proj.weight()], 0)
            .and_then(|x| x.contiguous())
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "fuse qkv weights", e))?;
        let qkv_proj = candle_nn::Linear::new(qkv_weight, None);
        let o_proj = candle_nn::linear_no_bias(
            cfg.num_attention_heads * cfg.head_dim,
            cfg.hidden_size,
            vb.pp("o_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load o_proj", e))?;

        let (query_layernorm, key_layernorm) = if cfg.use_qk_norm {
            let q_ln =
                candle_nn::rms_norm(cfg.head_dim, cfg.rms_norm_eps, vb.pp("query_layernorm"))
                    .map_err(|e| {
                        candle_to_ocr_inference("HunyuanOCR", "load query_layernorm", e)
                    })?;
            let k_ln = candle_nn::rms_norm(cfg.head_dim, cfg.rms_norm_eps, vb.pp("key_layernorm"))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load key_layernorm", e))?;
            (Some(q_ln), Some(k_ln))
        } else {
            (None, None)
        };

        // Cat-along-seq KV cache. Capacity 16384 covers ~1000-2000 vision tokens
        // plus the longest realistic generation. Same growth strategy as
        // candle_nn::kv_cache::KvCache (Tensor::cat per append).
        // Trim/gather support allows selective KV retention after generation steps.
        let kv_cache = TrimmableKvCache::new(2, DECODE_ROPE_CACHE_LEN);

        Ok(Self {
            qkv_proj,
            o_proj,
            query_layernorm,
            key_layernorm,
            num_heads: cfg.num_attention_heads,
            num_kv_heads: cfg.num_key_value_heads,
            num_kv_groups: cfg.num_attention_heads / cfg.num_key_value_heads,
            head_dim: cfg.head_dim,
            scaling: (cfg.head_dim as f64).powf(-0.5),
            kv_cache: RefCell::new(kv_cache),
        })
    }

    fn project_qkv(
        &self,
        hidden_states: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cos_sin: Option<&Tensor>,
    ) -> Result<(Tensor, Tensor, Tensor), Error> {
        #[cfg(not(feature = "cuda"))]
        let _ = cos_sin;
        let (b, seq_len, _) = hidden_states
            .dims3()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn hidden_states dims3", e))?;

        let qkv = self
            .qkv_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn qkv_proj", e))?;
        let q_width = self.num_heads * self.head_dim;
        let kv_width = self.num_kv_heads * self.head_dim;
        #[cfg(feature = "cuda")]
        let fused_qkv =
            if b == 1
                && self.head_dim == 128
                && qkv.device().is_cuda()
                && qkv.dtype() == candle_core::DType::BF16
                && cos.dtype() == candle_core::DType::F32
                && sin.dtype() == candle_core::DType::F32
                && cos_sin.is_some()
            {
                match (&self.query_layernorm, &self.key_layernorm) {
                    (Some(query_norm), Some(key_norm)) => {
                        let cos_sin = cos_sin.expect("checked above");
                        let q = qkv
                            .apply_op3_no_bwd(
                                cos_sin,
                                query_norm.weight(),
                                &FusedXdRopeRmsNormF16 {
                                    projection_width: q_width + 2 * kv_width,
                                    projection_offset: 0,
                                    num_heads: self.num_heads,
                                    query_len: seq_len,
                                    head_dim: self.head_dim,
                                    eps: query_norm.eps() as f32,
                                    include_v: false,
                                },
                            )
                            .map_err(|e| {
                                candle_to_ocr_inference("HunyuanOCR", "fused Q XDRoPE RMSNorm", e)
                            })?;
                        let kv = qkv
                            .apply_op3_no_bwd(
                                cos_sin,
                                key_norm.weight(),
                                &FusedXdRopeRmsNormF16 {
                                    projection_width: q_width + 2 * kv_width,
                                    projection_offset: q_width,
                                    num_heads: self.num_kv_heads,
                                    query_len: seq_len,
                                    head_dim: self.head_dim,
                                    eps: key_norm.eps() as f32,
                                    include_v: true,
                                },
                            )
                            .map_err(|e| {
                                candle_to_ocr_inference("HunyuanOCR", "fused KV XDRoPE RMSNorm", e)
                            })?;
                        let k = kv.narrow(0, 0, 1).and_then(|x| x.squeeze(0)).map_err(|e| {
                            candle_to_ocr_inference("HunyuanOCR", "fused K view", e)
                        })?;
                        let v = kv.narrow(0, 1, 1).and_then(|x| x.squeeze(0)).map_err(|e| {
                            candle_to_ocr_inference("HunyuanOCR", "fused V view", e)
                        })?;
                        Some((q, k, v))
                    }
                    _ => None,
                }
            } else {
                None
            };
        #[cfg(not(feature = "cuda"))]
        let fused_qkv: Option<(Tensor, Tensor, Tensor)> = None;
        if let Some(qkv) = fused_qkv {
            return Ok(qkv);
        }
        #[cfg(feature = "cuda")]
        let fused_qk = if b == 1
            && self.head_dim.is_multiple_of(2)
            && qkv.device().is_cuda()
            && qkv.dtype() == candle_core::DType::BF16
            && cos.dtype() == candle_core::DType::F32
            && sin.dtype() == candle_core::DType::F32
            && cos_sin.is_some()
        {
            let projection_width = q_width + 2 * kv_width;
            let cos_sin = cos_sin.expect("checked above");
            let q = Tensor::zeros(
                (b, self.num_heads, seq_len, self.head_dim),
                qkv.dtype(),
                qkv.device(),
            )
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "allocate fused XDRoPE Q", e))?;
            q.inplace_op3(
                &qkv,
                cos_sin,
                &FusedXdRope {
                    projection_width,
                    projection_offset: 0,
                    num_heads: self.num_heads,
                    query_len: seq_len,
                    head_dim: self.head_dim,
                },
            )
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "fused XDRoPE Q", e))?;
            let k = Tensor::zeros(
                (b, self.num_kv_heads, seq_len, self.head_dim),
                qkv.dtype(),
                qkv.device(),
            )
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "allocate fused XDRoPE K", e))?;
            k.inplace_op3(
                &qkv,
                cos_sin,
                &FusedXdRope {
                    projection_width,
                    projection_offset: q_width,
                    num_heads: self.num_kv_heads,
                    query_len: seq_len,
                    head_dim: self.head_dim,
                },
            )
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "fused XDRoPE K", e))?;
            Some((q, k))
        } else {
            None
        };
        #[cfg(not(feature = "cuda"))]
        let fused_qk: Option<(Tensor, Tensor)> = None;

        let (q, k) = match fused_qk {
            Some(qk) => qk,
            None => {
                let q = qkv
                    .narrow(2, 0, q_width)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn q slice", e))?
                    .reshape((b, seq_len, self.num_heads, self.head_dim))
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn q reshape", e))?
                    .transpose(1, 2)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn q transpose", e))?;
                let k = qkv
                    .narrow(2, q_width, kv_width)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn k slice", e))?
                    .reshape((b, seq_len, self.num_kv_heads, self.head_dim))
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn k reshape", e))?
                    .transpose(1, 2)
                    .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn k transpose", e))?;
                apply_xdrope_rotary_pos_emb(&q, &k, cos, sin)?
            }
        };

        let v = qkv
            .narrow(2, q_width + kv_width, kv_width)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn v slice", e))?
            .reshape((b, seq_len, self.num_kv_heads, self.head_dim))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn v reshape", e))?
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn v transpose", e))?;

        // Match upstream HunyuanVL: apply XDRoPE first, then Q/K RMSNorm.
        // The learned RMSNorm weight is per head dimension, so it does not
        // commute with the rotary half-dimension mixing.
        let q = match &self.query_layernorm {
            Some(ln) => ln
                .forward(&q)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn query_layernorm", e))?,
            None => q,
        };
        let k = match &self.key_layernorm {
            Some(ln) => ln
                .forward(&k)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn key_layernorm", e))?,
            None => k,
        };

        let q = q
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn q contiguous", e))?;
        let k = k
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn k contiguous", e))?;
        let v = v
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn v contiguous", e))?;
        // Keep the model in BF16 but use FP16 inside CUDA attention. The
        // FlashAttention BF16 verification path can amplify tiny per-round
        // differences into a different greedy branch on long documents. Only
        // BF16 gets downcast this way: an explicit `OAR_VL_DTYPE=f32`
        // override is meant to raise precision, not get silently discarded.
        let attention_dtype = if q.device().is_cuda() && q.dtype() == candle_core::DType::BF16 {
            candle_core::DType::F16
        } else {
            q.dtype()
        };
        let q = q
            .to_dtype(attention_dtype)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn q attention dtype", e))?;
        let k = k
            .to_dtype(attention_dtype)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn k attention dtype", e))?;
        let v = v
            .to_dtype(attention_dtype)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn v attention dtype", e))?;

        Ok((q, k, v))
    }

    fn attend_projected(
        &self,
        q: &Tensor,
        k: &Tensor,
        v: &Tensor,
        model_dtype: candle_core::DType,
        causal_mask: Option<&Tensor>,
    ) -> Result<Tensor, Error> {
        let (b, _, seq_len, _) = q
            .dims4()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "projected Q dims", e))?;

        let (k_all, v_all) = self
            .kv_cache
            .borrow_mut()
            .append(k, v)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "append kv_cache", e))?;

        // Single-document inference has no padding, so FlashAttention can
        // replace the explicit causal mask and consume GQA K/V directly.
        // Batched/padded and non-CUDA paths retain the portable eager kernel.
        let flash_output = if b == 1 {
            flash_attention(q, &k_all, &v_all, self.scaling, seq_len > 1)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "flash attention", e))?
        } else {
            None
        };
        let attn_output = match flash_output {
            Some(output) => output,
            None => scaled_dot_product_attention_gqa(
                q,
                &k_all,
                &v_all,
                causal_mask,
                self.scaling,
                seq_len > 1,
                self.num_kv_groups,
            )
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "grouped-query attention", e))?,
        };
        self.project_attention_output(&attn_output, model_dtype, b, seq_len)
    }

    fn project_attention_output(
        &self,
        attn_output: &Tensor,
        model_dtype: candle_core::DType,
        batch: usize,
        seq_len: usize,
    ) -> Result<Tensor, Error> {
        let attn_output = attn_output
            .to_dtype(model_dtype)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn output model dtype", e))?
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn out transpose", e))?
            .reshape((batch, seq_len, self.num_heads * self.head_dim))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn out reshape", e))?;

        self.o_proj
            .forward(&attn_output)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "attn o_proj", e))
    }

    #[cfg(feature = "cuda")]
    fn prepare_dynamic_cache(&self, query_len: usize, cache_len: usize) -> Result<(), Error> {
        let device = self.qkv_proj.weight().device();
        let template = Tensor::zeros(
            (1, self.num_kv_heads, query_len, self.head_dim),
            candle_core::DType::F16,
            device,
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic KV template", e))?;
        self.kv_cache
            .borrow_mut()
            .initialize_storage_with_capacity(&template, cache_len)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "initialize dynamic KV", e))
    }

    #[cfg(feature = "cuda")]
    fn attend_projected_dynamic(
        &self,
        q: &Tensor,
        k: &Tensor,
        v: &Tensor,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
        model_dtype: candle_core::DType,
    ) -> Result<Tensor, Error> {
        let (batch, _, query_len, _) = q
            .dims4()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic Q dims", e))?;
        if batch != 1 {
            return Err(Error::Config {
                message: "HunyuanOCR dynamic CUDA-graph attention requires batch size 1"
                    .to_string(),
            });
        }
        let cache = self.kv_cache.borrow();
        let cache_len = cache.storage_capacity();
        let (cache_k, cache_v) = cache.storage().ok_or_else(|| Error::Config {
            message: "HunyuanOCR dynamic KV storage is not initialized".to_string(),
        })?;
        drop(cache);
        let append = DynamicKvAppend {
            query_len,
            cache_len,
        };
        cache_k
            .inplace_op3(k, kv_lengths, &append)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic key cache append", e))?;
        cache_v
            .inplace_op3(v, kv_lengths, &append)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic value cache append", e))?;

        let q = q
            .squeeze(0)
            .and_then(|x| x.transpose(0, 1))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic Q layout", e))?;
        let cache_k = cache_k
            .squeeze(0)
            .and_then(|x| x.transpose(0, 1))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic K layout", e))?;
        let cache_v = cache_v
            .squeeze(0)
            .and_then(|x| x.transpose(0, 1))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic V layout", e))?;
        let output = candle_flash_attn::flash_attn_varlen(
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
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic flash attention", e))?
        .transpose(0, 1)
        .and_then(|x| x.unsqueeze(0))
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic attention layout", e))?;
        self.project_attention_output(&output, model_dtype, batch, query_len)
    }

    fn clear_kv_cache(&self) {
        self.kv_cache.borrow_mut().reset();
    }

    fn trim_kv_cache(&self, len: usize) -> Result<(), Error> {
        self.kv_cache
            .borrow_mut()
            .trim_to(len)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "trim kv_cache", e))
    }

    fn kv_cache_len(&self) -> usize {
        self.kv_cache.borrow().current_seq_len()
    }

    #[cfg(feature = "cuda")]
    fn set_kv_cache_len(&self, len: usize) -> Result<(), Error> {
        self.kv_cache
            .borrow_mut()
            .set_current_len(len)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "set dynamic KV length", e))
    }
}

#[cfg(feature = "cuda")]
struct TargetDecoderCudaGraph {
    // The executable graph owns raw pointers into every tensor below and into
    // the decoder weights; dispose via `dispose` so capture-touched buffers
    // are never returned to the stream-ordered allocator (see
    // SingleTokenDecoderCudaGraph::dispose).
    graph: candle_core::cuda_backend::cudarc::driver::CudaGraph,
    hidden_input: Tensor,
    cos_input: Tensor,
    sin_input: Tensor,
    _query_lengths: Tensor,
    kv_lengths: CudaGraphKvLengths,
    hidden_output: Tensor,
    logits_output: Tensor,
    aux_output: Option<Tensor>,
    aux_layer_ids: Vec<usize>,
    cache_len: usize,
}

#[cfg(feature = "cuda")]
impl TargetDecoderCudaGraph {
    fn dispose(self) {
        let Self {
            graph,
            hidden_input,
            cos_input,
            sin_input,
            _query_lengths,
            kv_lengths,
            hidden_output,
            logits_output,
            aux_output,
            aux_layer_ids: _,
            cache_len: _,
        } = self;
        let device = hidden_input.device().clone();
        report_stashed_cuda_error(&device, "CUDA graph disposal");
        drop_and_drain(graph, &device);
        drop_and_drain(aux_output, &device);
        drop_and_drain(logits_output, &device);
        drop_and_drain(hidden_output, &device);
        drop_and_drain(kv_lengths, &device);
        drop_and_drain(_query_lengths, &device);
        drop_and_drain(sin_input, &device);
        drop_and_drain(cos_input, &device);
        drop_and_drain(hidden_input, &device);
    }
}

#[cfg(feature = "cuda")]
impl std::fmt::Debug for TargetDecoderCudaGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TargetDecoderCudaGraph")
            .field("hidden", &self.hidden_input.shape())
            .field("aux_layer_ids", &self.aux_layer_ids)
            .field("cache_len", &self.cache_len)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct HunyuanDecoderLayer {
    self_attn: HunyuanAttention,
    mlp: HunyuanMlp,
    input_layernorm: candle_nn::RmsNorm,
    post_attention_layernorm: candle_nn::RmsNorm,
}

impl HunyuanDecoderLayer {
    fn load(cfg: &HunyuanOcrConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let self_attn = HunyuanAttention::load(cfg, vb.pp("self_attn"))?;
        let mlp = HunyuanMlp::load(cfg, vb.pp("mlp"))?;
        let input_layernorm =
            candle_nn::rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("input_layernorm"))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load input_layernorm", e))?;
        let post_attention_layernorm = candle_nn::rms_norm(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            vb.pp("post_attention_layernorm"),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load post_attention_layernorm", e))?;
        Ok(Self {
            self_attn,
            mlp,
            input_layernorm,
            post_attention_layernorm,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cos_sin: Option<&Tensor>,
        causal_mask: Option<&Tensor>,
    ) -> Result<Tensor, Error> {
        let normalized = self
            .input_layernorm
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "llm input_layernorm", e))?;
        let (q, k, v) = self.self_attn.project_qkv(&normalized, cos, sin, cos_sin)?;
        let attention =
            self.self_attn
                .attend_projected(&q, &k, &v, hidden_states.dtype(), causal_mask)?;
        self.post_attention_eager(hidden_states, &attention)
    }

    #[cfg(feature = "cuda")]
    fn forward_dynamic(
        &self,
        hidden_states: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        cos_sin: Option<&Tensor>,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
    ) -> Result<Tensor, Error> {
        let normalized = self
            .input_layernorm
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "llm input_layernorm", e))?;
        let (q, k, v) = self.self_attn.project_qkv(&normalized, cos, sin, cos_sin)?;
        let attention = self.self_attn.attend_projected_dynamic(
            &q,
            &k,
            &v,
            query_lengths,
            kv_lengths,
            hidden_states.dtype(),
        )?;
        self.post_attention_eager(hidden_states, &attention)
    }

    fn post_attention_eager(
        &self,
        hidden_states: &Tensor,
        attention: &Tensor,
    ) -> Result<Tensor, Error> {
        #[cfg(feature = "cuda")]
        if hidden_states.device().is_cuda()
            && hidden_states.dtype() == candle_core::DType::BF16
            && hidden_states.is_contiguous()
            && attention.is_contiguous()
        {
            let packed = hidden_states
                .apply_op3_no_bwd(
                    attention,
                    self.post_attention_layernorm.weight(),
                    &FusedAddRmsNormBf16 {
                        eps: self.post_attention_layernorm.eps() as f32,
                    },
                )
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "fused residual RMSNorm", e))?;
            let residual = packed
                .narrow(0, 0, 1)
                .and_then(|x| x.squeeze(0))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "fused residual view", e))?;
            let normalized = packed
                .narrow(0, 1, 1)
                .and_then(|x| x.squeeze(0))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "fused RMSNorm view", e))?;
            let mlp_out = self.mlp.forward(&normalized)?;
            return (&residual + &mlp_out)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "llm mlp residual add", e));
        }
        let hidden_states = (hidden_states + attention)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "llm attn residual add", e))?;
        let residual = hidden_states.clone();
        let normalized = self
            .post_attention_layernorm
            .forward(&hidden_states)
            .map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "llm post_attention_layernorm", e)
            })?;
        let mlp_out = self.mlp.forward(&normalized)?;
        (&residual + &mlp_out)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "llm mlp residual add", e))
    }

    fn clear_kv_cache(&self) {
        self.self_attn.clear_kv_cache();
    }

    fn trim_kv_cache(&self, len: usize) -> Result<(), Error> {
        self.self_attn.trim_kv_cache(len)
    }

    fn kv_cache_len(&self) -> usize {
        self.self_attn.kv_cache_len()
    }

    #[cfg(feature = "cuda")]
    fn set_kv_cache_len(&self, len: usize) -> Result<(), Error> {
        self.self_attn.set_kv_cache_len(len)
    }
}

/// Target-model outputs needed by DFlash. `hidden_states` is the regular
/// post-norm decoder output. `aux_hidden_states` concatenates the requested
/// intermediate layer outputs along the hidden dimension, matching vLLM's
/// DFlash target interface.
pub(crate) struct HunyuanLlmOutput {
    pub hidden_states: Tensor,
    /// LM-head output is populated when decode runs through a CUDA graph.
    /// Prefill/eager paths leave it absent to avoid projecting every prompt
    /// position into the full vocabulary.
    pub logits: Option<Tensor>,
    pub aux_hidden_states: Option<Tensor>,
}

#[derive(Debug)]
pub struct HunyuanLlm {
    #[cfg(feature = "cuda")]
    decode_graph: RefCell<Option<TargetDecoderCudaGraph>>,
    embed_tokens: candle_nn::Embedding,
    layers: Vec<HunyuanDecoderLayer>,
    norm: candle_nn::RmsNorm,
    rotary: RotaryEmbedding,
    decode_cos: Tensor,
    decode_sin: Tensor,
    /// XDRoPE section sizes (`config.rope_scaling.xdrope_section`). Used once
    /// per `forward` to section-mix the rotary `cos`/`sin` tensors before they
    /// fan out to every layer — see [`apply_xdrope_rotary_pos_emb`].
    xdrope_section: Vec<usize>,
    // Must stay the last field: it drops last and drains CUDA errors the
    // other fields' frees may stash (see CudaGraphDrainGuard).
    #[cfg(feature = "cuda")]
    _drain_guard: CudaGraphDrainGuard,
}

impl HunyuanLlm {
    pub fn load(cfg: &HunyuanOcrConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let embed_tokens =
            candle_nn::embedding(cfg.vocab_size, cfg.hidden_size, vb.pp("embed_tokens"))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load embed_tokens", e))?;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            layers.push(HunyuanDecoderLayer::load(
                cfg,
                vb.pp(format!("layers.{i}")),
            )?);
        }
        let norm = candle_nn::rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm"))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "load final norm", e))?;

        let rope_theta = match cfg.rope_scaling.alpha {
            Some(alpha) if alpha != 0.0 => {
                cfg.rope_theta * alpha.powf(cfg.head_dim as f64 / (cfg.head_dim as f64 - 2.0))
            }
            _ => cfg.rope_theta,
        };
        let rotary = RotaryEmbedding::new_multi_axis(cfg.head_dim, rope_theta, 4, vb.device())?;
        let decode_rotary = RotaryEmbedding::new_dynamic(cfg.head_dim, rope_theta, vb.device())?;
        let decode_positions = Tensor::arange(0i64, DECODE_ROPE_CACHE_LEN as i64, vb.device())
            .and_then(|x| x.reshape((1, 1, DECODE_ROPE_CACHE_LEN)))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "decode rope positions", e))?;
        let (decode_cos, decode_sin) =
            decode_rotary.forward_multi_axis(&decode_positions, vb.dtype())?;
        // Decode positions use the same scalar on every XDRoPE axis, so the
        // section selection collapses to ordinary 1-D RoPE. Preserve the
        // official BF16 quantization point, then hoist the per-round F32 cast.
        let decode_cos = decode_cos
            .to_dtype(candle_core::DType::F32)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "decode rope cos f32", e))?;
        let decode_sin = decode_sin
            .to_dtype(candle_core::DType::F32)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "decode rope sin f32", e))?;

        #[cfg(feature = "cuda")]
        let _drain_guard = CudaGraphDrainGuard::new(vb.device());
        Ok(Self {
            #[cfg(feature = "cuda")]
            decode_graph: RefCell::new(None),
            embed_tokens,
            layers,
            norm,
            rotary,
            decode_cos,
            decode_sin,
            xdrope_section: cfg.rope_scaling.xdrope_section.clone(),
            #[cfg(feature = "cuda")]
            _drain_guard,
        })
    }

    pub fn embed(&self, input_ids: &Tensor) -> Result<Tensor, Error> {
        self.embed_tokens
            .forward(input_ids)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "embed tokens", e))
    }

    pub fn token_embedding_weight(&self) -> Tensor {
        self.embed_tokens.embeddings().clone()
    }

    pub fn forward(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
        causal_mask: Option<&Tensor>,
    ) -> Result<Tensor, Error> {
        Ok(self
            .forward_with_aux(inputs_embeds, position_ids, causal_mask, &[])?
            .hidden_states)
    }

    /// Forward the target decoder and optionally retain intermediate features.
    ///
    /// Layer ids are one-based boundary ids: id `1` is the output after the
    /// first decoder layer. DFlash checkpoint ids are zero-based layer indices,
    /// so callers must add one before passing them here (as vLLM does).
    pub(crate) fn forward_with_aux(
        &self,
        inputs_embeds: &Tensor,
        position_ids: &Tensor,
        causal_mask: Option<&Tensor>,
        aux_layer_ids: &[usize],
    ) -> Result<HunyuanLlmOutput, Error> {
        use candle_core::DType;

        let (cos, sin) = self
            .rotary
            .forward_multi_axis(position_ids, inputs_embeds.dtype())?;

        // XDRoPE section-mix + F32 cast: the result is layer-invariant
        // (depends only on position_ids and xdrope_section), so do it once
        // here instead of per-layer inside `apply_xdrope_rotary_pos_emb`.
        // Saves ~num_layers × (2 select_rope_sections + 2 to_dtype) ops per
        // forward.
        let cos = select_rope_sections(&cos, &self.xdrope_section, 4)?
            .to_dtype(DType::F32)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "xdrope cos to_dtype f32", e))?;
        let sin = select_rope_sections(&sin, &self.xdrope_section, 4)?
            .to_dtype(DType::F32)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "xdrope sin to_dtype f32", e))?;

        self.forward_with_aux_prepared(inputs_embeds, &cos, &sin, causal_mask, aux_layer_ids)
    }

    pub(crate) fn forward_with_aux_decode(
        &self,
        inputs_embeds: &Tensor,
        start: usize,
        causal_mask: Option<&Tensor>,
        aux_layer_ids: &[usize],
    ) -> Result<HunyuanLlmOutput, Error> {
        let len = inputs_embeds
            .dim(1)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "decode sequence length", e))?;
        if start + len > DECODE_ROPE_CACHE_LEN {
            #[cfg(feature = "cuda")]
            self.invalidate_cuda_graph();
            let positions: Vec<i64> = (start..start + len)
                .flat_map(|position| [position as i64; 4])
                .collect();
            let positions = Tensor::new(positions, inputs_embeds.device())
                .and_then(|t| t.reshape((len, 4)))
                .and_then(|t| t.transpose(0, 1))
                .and_then(|t| t.unsqueeze(1))
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "decode positions", e))?;
            return self.forward_with_aux(inputs_embeds, &positions, causal_mask, aux_layer_ids);
        }
        let cos = self
            .decode_cos
            .narrow(2, start, len)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "decode rope cos slice", e))?;
        let sin = self
            .decode_sin
            .narrow(2, start, len)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "decode rope sin slice", e))?;
        #[cfg(feature = "cuda")]
        if let Some(output) =
            self.replay_cuda_graph(inputs_embeds, &cos, &sin, start + len, aux_layer_ids)?
        {
            return Ok(output);
        }
        self.forward_with_aux_prepared(inputs_embeds, &cos, &sin, causal_mask, aux_layer_ids)
    }

    #[cfg(feature = "cuda")]
    fn replay_cuda_graph(
        &self,
        inputs_embeds: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        kv_len: usize,
        aux_layer_ids: &[usize],
    ) -> Result<Option<HunyuanLlmOutput>, Error> {
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
            || cos.shape() != captured.cos_input.shape()
            || sin.shape() != captured.sin_input.shape()
            || aux_layer_ids != captured.aux_layer_ids
        {
            return Ok(None);
        }
        captured
            .hidden_input
            .slice_set(inputs_embeds, 0, 0)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "copy full graph hidden", e))?;
        captured
            .cos_input
            .slice_set(cos, 0, 0)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "copy full graph cos", e))?;
        captured
            .sin_input
            .slice_set(sin, 0, 0)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "copy full graph sin", e))?;
        captured.kv_lengths.update(kv_len).map_err(|e| {
            candle_to_ocr_inference("HunyuanOCR", "update full graph KV lengths", e)
        })?;
        captured
            .graph
            .launch()
            .map_err(|e| cuda_graph_error("HunyuanOCR", "launch full target decoder graph", e))?;
        for layer in &self.layers {
            layer.set_kv_cache_len(kv_len)?;
        }
        Ok(Some(HunyuanLlmOutput {
            hidden_states: captured.hidden_output.clone(),
            logits: Some(captured.logits_output.clone()),
            aux_hidden_states: captured.aux_output.clone(),
        }))
    }

    fn forward_with_aux_prepared(
        &self,
        inputs_embeds: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        causal_mask: Option<&Tensor>,
        aux_layer_ids: &[usize],
    ) -> Result<HunyuanLlmOutput, Error> {
        #[cfg(feature = "cuda")]
        {
            let incoming_len = inputs_embeds.dim(1).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "prepared sequence length", e)
            })?;
            let required = self.kv_cache_len().saturating_add(incoming_len);
            let graph_capacity = self
                .decode_graph
                .borrow()
                .as_ref()
                .map(|graph| graph.cache_len);
            if graph_capacity.is_some_and(|capacity| required > capacity) {
                self.invalidate_cuda_graph();
            }
        }
        // Decode invokes the same non-contiguous elementwise layouts thousands
        // of times. Candle otherwise uploads their dims/strides for every
        // kernel. Restrict the built-in HtoD cache to the pure model forward so
        // dynamic token tensors created by the caller are never retained.
        #[cfg(feature = "cuda")]
        let _cuda_htod_cache = match inputs_embeds.device() {
            Device::Cuda(device) => Some(device.enable_cuda_graph_htod_cache()),
            _ => None,
        };
        if aux_layer_ids
            .iter()
            .any(|&id| id == 0 || id > self.layers.len())
        {
            return Err(Error::Config {
                message: format!(
                    "HunyuanOCR: DFlash target layer ids must be in 1..={}, got {:?}",
                    self.layers.len(),
                    aux_layer_ids
                ),
            });
        }
        let mut hidden_states = inputs_embeds.clone();
        #[cfg(feature = "cuda")]
        let cos_sin = if inputs_embeds.device().is_cuda()
            && inputs_embeds.dtype() == candle_core::DType::BF16
        {
            Some(Tensor::cat(&[cos, sin], 0).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "pack decoder XDRoPE cos/sin", e)
            })?)
        } else {
            None
        };
        #[cfg(not(feature = "cuda"))]
        let cos_sin: Option<Tensor> = None;
        let mut aux = Vec::with_capacity(aux_layer_ids.len());
        for (index, layer) in self.layers.iter().enumerate() {
            hidden_states =
                layer.forward(&hidden_states, cos, sin, cos_sin.as_ref(), causal_mask)?;
            if aux_layer_ids.contains(&(index + 1)) {
                aux.push(hidden_states.clone());
            }
        }
        let aux_hidden_states = if aux.is_empty() {
            None
        } else {
            let refs: Vec<&Tensor> = aux.iter().collect();
            Some(Tensor::cat(&refs, 2).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "concatenate DFlash target features", e)
            })?)
        };
        let hidden_states = self
            .norm
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "llm final norm", e))?;
        Ok(HunyuanLlmOutput {
            hidden_states,
            logits: None,
            aux_hidden_states,
        })
    }

    #[cfg(feature = "cuda")]
    fn forward_with_aux_dynamic(
        &self,
        inputs_embeds: &Tensor,
        cos: &Tensor,
        sin: &Tensor,
        query_lengths: &Tensor,
        kv_lengths: &Tensor,
        aux_layer_ids: &[usize],
    ) -> Result<HunyuanLlmOutput, Error> {
        let mut hidden_states = inputs_embeds.clone();
        let cos_sin = if inputs_embeds.dtype() == candle_core::DType::BF16 {
            Some(Tensor::cat(&[cos, sin], 0).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "pack dynamic XDRoPE cos/sin", e)
            })?)
        } else {
            None
        };
        let mut aux = Vec::with_capacity(aux_layer_ids.len());
        for (index, layer) in self.layers.iter().enumerate() {
            hidden_states = layer.forward_dynamic(
                &hidden_states,
                cos,
                sin,
                cos_sin.as_ref(),
                query_lengths,
                kv_lengths,
            )?;
            if aux_layer_ids.contains(&(index + 1)) {
                aux.push(hidden_states.clone());
            }
        }
        let aux_hidden_states = if aux.is_empty() {
            None
        } else {
            let refs: Vec<&Tensor> = aux.iter().collect();
            Some(Tensor::cat(&refs, 2).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "dynamic target auxiliary states", e)
            })?)
        };
        let hidden_states = self
            .norm
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "dynamic target final norm", e))?;
        Ok(HunyuanLlmOutput {
            hidden_states,
            logits: None,
            aux_hidden_states,
        })
    }

    #[cfg(feature = "cuda")]
    fn project_logits(&self, hidden_states: &Tensor) -> Result<Tensor, Error> {
        let (batch_size, sequence_length, hidden_size) = hidden_states
            .dims3()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "graph hidden shape", e))?;
        let weight_t = self
            .embed_tokens
            .embeddings()
            .transpose(0, 1)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "transpose LM head", e))?;
        let vocab_size = weight_t
            .dim(1)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "LM head vocab size", e))?;
        hidden_states
            .reshape((batch_size * sequence_length, hidden_size))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "flatten graph hidden", e))?
            .matmul(&weight_t)
            .and_then(|logits| logits.reshape((batch_size, sequence_length, vocab_size)))
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "project graph logits", e))
    }

    pub fn clear_kv_cache(&self) {
        for layer in &self.layers {
            layer.clear_kv_cache();
        }
    }

    pub(crate) fn prepare_dflash_cuda_graphs(
        &self,
        query_len: usize,
        aux_layer_ids: &[usize],
    ) -> Result<(), Error> {
        if std::env::var_os("OAR_HUNYUAN_DISABLE_CUDA_GRAPH").is_some() {
            #[cfg(feature = "cuda")]
            self.invalidate_cuda_graph();
            return Ok(());
        }
        // Dynamic target attention pins an F16 KV cache for graph replay.
        // F32 keeps its higher-precision eager path instead of failing capture
        // when the append kernel sees F32 Q/K/V and F16 cache storage.
        #[cfg(feature = "cuda")]
        if self.decode_cos.device().is_cuda()
            && target_cuda_graph_dtype_supported(self.embed_tokens.embeddings().dtype())
        {
            let reusable = self.decode_graph.borrow().as_ref().is_some_and(|graph| {
                graph.hidden_input.dim(1).ok() == Some(query_len)
                    && graph.cache_len == DECODE_ROPE_CACHE_LEN
                    && graph.aux_layer_ids == aux_layer_ids
            });
            if reusable {
                return Ok(());
            }
            self.invalidate_cuda_graph();
            let cos = self.decode_cos.narrow(2, 0, query_len).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "graph decode cos template", e)
            })?;
            let sin = self.decode_sin.narrow(2, 0, query_len).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "graph decode sin template", e)
            })?;
            self.capture_cuda_graph(query_len, DECODE_ROPE_CACHE_LEN, &cos, &sin, aux_layer_ids)?;
        }
        let _ = query_len;
        let _ = aux_layer_ids;
        Ok(())
    }

    /// Lazily capture the single-token autoregressive decoder. Capacity is
    /// selected per workload and rounded up so short requests do not reserve
    /// the full 16K-token graph cache. A later larger request transparently
    /// recaptures with a larger bucket.
    pub(crate) fn prepare_ar_cuda_graph(
        &self,
        prompt_len: usize,
        max_new_tokens: usize,
    ) -> Result<(), Error> {
        if std::env::var_os("OAR_HUNYUAN_DISABLE_CUDA_GRAPH").is_some()
            || std::env::var_os("OAR_HUNYUAN_DISABLE_AR_CUDA_GRAPH").is_some()
        {
            #[cfg(feature = "cuda")]
            self.invalidate_cuda_graph();
            return Ok(());
        }
        if max_new_tokens == 0 {
            return Ok(());
        }
        #[cfg(feature = "cuda")]
        if self.decode_cos.device().is_cuda()
            && target_cuda_graph_dtype_supported(self.embed_tokens.embeddings().dtype())
        {
            let Some(cache_len) = ar_cuda_graph_capacity(prompt_len, max_new_tokens) else {
                self.invalidate_cuda_graph();
                return Ok(());
            };
            let required = prompt_len
                .saturating_add(max_new_tokens)
                .min(DECODE_ROPE_CACHE_LEN);
            let reusable = self.decode_graph.borrow().as_ref().is_some_and(|graph| {
                graph.hidden_input.dim(1).ok() == Some(1)
                    && graph.cache_len >= required
                    && graph.aux_layer_ids.is_empty()
            });
            if reusable {
                return Ok(());
            }
            self.invalidate_cuda_graph();
            let cos = self.decode_cos.narrow(2, 0, 1).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "AR graph decode cos template", e)
            })?;
            let sin = self.decode_sin.narrow(2, 0, 1).map_err(|e| {
                candle_to_ocr_inference("HunyuanOCR", "AR graph decode sin template", e)
            })?;
            self.capture_cuda_graph(1, cache_len, &cos, &sin, &[])?;
        }
        let _ = prompt_len;
        let _ = max_new_tokens;
        Ok(())
    }

    #[cfg(feature = "cuda")]
    fn capture_cuda_graph(
        &self,
        query_len: usize,
        cache_len: usize,
        cos_template: &Tensor,
        sin_template: &Tensor,
        aux_layer_ids: &[usize],
    ) -> Result<(), Error> {
        use candle_core::cuda_backend::cudarc::driver::sys::{
            CUgraphInstantiate_flags_enum, CUstreamCaptureMode_enum,
        };

        if self.decode_graph.borrow().is_some() {
            return Ok(());
        }
        let Device::Cuda(cuda) = self.decode_cos.device() else {
            return Ok(());
        };
        for layer in &self.layers {
            layer
                .self_attn
                .prepare_dynamic_cache(query_len, cache_len)?;
        }
        let hidden_size = self
            .embed_tokens
            .embeddings()
            .dim(1)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "graph hidden size", e))?;
        let dtype = self.embed_tokens.embeddings().dtype();
        let hidden_input =
            Tensor::zeros((1, query_len, hidden_size), dtype, self.decode_cos.device())
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "full graph hidden input", e))?;
        let cos_input = Tensor::zeros(
            cos_template.shape(),
            cos_template.dtype(),
            self.decode_cos.device(),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "full graph cos input", e))?;
        let sin_input = Tensor::zeros(
            sin_template.shape(),
            sin_template.dtype(),
            self.decode_sin.device(),
        )
        .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "full graph sin input", e))?;
        let query_lengths = Tensor::new(&[0u32, query_len as u32], self.decode_cos.device())
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "full graph query lengths", e))?;
        // These must not share storage: query length stays fixed while
        // the cumulative KV length is overwritten before every graph replay.
        let kv_lengths = CudaGraphKvLengths::new(query_len, self.decode_cos.device())
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "full graph KV lengths", e))?;
        let stream = cuda.cuda_stream();
        let _htod_cache = cuda.enable_cuda_graph_htod_cache();

        let warm = self.forward_with_aux_dynamic(
            &hidden_input,
            &cos_input,
            &sin_input,
            &query_lengths,
            kv_lengths.tensor(),
            aux_layer_ids,
        )?;
        let warm_logits = self.project_logits(&warm.hidden_states)?;
        sync_graph_tensor("HunyuanOCR", &warm_logits, "warm full target decoder graph")?;
        // Allocate the output buffers before capture so they belong to the
        // regular stream-ordered pool; a capture-time allocation lives in the
        // graph's private pool and can never be returned to the allocator
        // safely. Prime the copies so the captured run sees warm kernels.
        let hidden_output = Tensor::zeros_like(&warm.hidden_states)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "full graph hidden output", e))?;
        let logits_output = Tensor::zeros_like(&warm_logits)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "full graph logits output", e))?;
        let aux_output = warm
            .aux_hidden_states
            .as_ref()
            .map(Tensor::zeros_like)
            .transpose()
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "full graph aux output", e))?;
        hidden_output
            .slice_set(&warm.hidden_states, 0, 0)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "prime full hidden copy", e))?;
        logits_output
            .slice_set(&warm_logits, 0, 0)
            .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "prime full logits copy", e))?;
        if let (Some(buffer), Some(source)) = (&aux_output, &warm.aux_hidden_states) {
            buffer
                .slice_set(source, 0, 0)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "prime full aux copy", e))?;
        }

        stream
            .begin_capture(CUstreamCaptureMode_enum::CU_STREAM_CAPTURE_MODE_GLOBAL)
            .map_err(|e| {
                cuda_graph_error("HunyuanOCR", "begin full target decoder graph capture", e)
            })?;
        let captured_output: Result<(), Error> = (|| {
            let output = self.forward_with_aux_dynamic(
                &hidden_input,
                &cos_input,
                &sin_input,
                &query_lengths,
                kv_lengths.tensor(),
                aux_layer_ids,
            )?;
            let logits = self.project_logits(&output.hidden_states)?;
            hidden_output
                .slice_set(&output.hidden_states, 0, 0)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "record full hidden copy", e))?;
            logits_output
                .slice_set(&logits, 0, 0)
                .map_err(|e| candle_to_ocr_inference("HunyuanOCR", "record full logits copy", e))?;
            if let (Some(buffer), Some(source)) = (&aux_output, &output.aux_hidden_states) {
                buffer.slice_set(source, 0, 0).map_err(|e| {
                    candle_to_ocr_inference("HunyuanOCR", "record full aux copy", e)
                })?;
            }
            Ok(())
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
            .map_err(|e| {
                cuda_graph_error("HunyuanOCR", "end full target decoder graph capture", e)
            })?
            .ok_or_else(|| Error::Config {
                message: "HunyuanOCR full target decoder capture returned no graph".to_string(),
            })?;
        graph
            .launch()
            .map_err(|e| cuda_graph_error("HunyuanOCR", "warm full target decoder graph", e))?;
        sync_graph_tensor(
            "HunyuanOCR",
            &logits_output,
            "sync full target decoder graph",
        )?;
        self.clear_kv_cache();
        *self.decode_graph.borrow_mut() = Some(TargetDecoderCudaGraph {
            graph,
            hidden_input,
            cos_input,
            sin_input,
            _query_lengths: query_lengths,
            kv_lengths,
            hidden_output,
            logits_output,
            aux_output,
            aux_layer_ids: aux_layer_ids.to_vec(),
            cache_len,
        });
        Ok(())
    }

    #[cfg(feature = "cuda")]
    fn invalidate_cuda_graph(&self) {
        // Eager cache growth reallocates the storage whose raw pointers were
        // captured by the graph. Dropping it before growth also prevents a
        // stale replay after the logical cache is reset for another document.
        if let Some(graph) = self.decode_graph.borrow_mut().take() {
            graph.dispose();
        }
    }

    /// Drop any captured target-decoder CUDA graph.
    ///
    /// Callers must invoke this before feeding a batch shape the graph
    /// wasn't captured for (e.g. a multi-image request that disables DFlash
    /// and falls back to plain batched prefill): the batched append would
    /// otherwise reallocate the per-layer KV storage the graph's raw device
    /// pointers still reference, and a later single-image DFlash decode
    /// could replay the graph against that freed memory.
    pub(crate) fn invalidate_target_cuda_graph(&self) {
        #[cfg(feature = "cuda")]
        self.invalidate_cuda_graph();
    }

    pub(crate) fn trim_kv_cache(&self, len: usize) -> Result<(), Error> {
        for layer in &self.layers {
            layer.trim_kv_cache(len)?;
        }
        Ok(())
    }

    pub(crate) fn kv_cache_len(&self) -> usize {
        self.layers
            .first()
            .map_or(0, HunyuanDecoderLayer::kv_cache_len)
    }
}

#[cfg(feature = "cuda")]
impl Drop for HunyuanLlm {
    fn drop(&mut self) {
        // A cached graph must go through dispose: plainly dropping it returns
        // graph-bound buffers to the allocator and poisons it.
        self.invalidate_cuda_graph();
    }
}

#[cfg(test)]
mod tests {
    use super::{DECODE_ROPE_CACHE_LEN, ar_cuda_graph_capacity, target_cuda_graph_dtype_supported};
    use candle_core::DType;

    #[test]
    fn target_cuda_graph_skips_unsupported_dtypes() {
        assert!(target_cuda_graph_dtype_supported(DType::F16));
        assert!(target_cuda_graph_dtype_supported(DType::BF16));
        assert!(!target_cuda_graph_dtype_supported(DType::F32));
        assert!(!target_cuda_graph_dtype_supported(DType::F64));
    }

    #[test]
    fn ar_graph_capacity_uses_bounded_power_of_two_buckets() {
        assert_eq!(ar_cuda_graph_capacity(1500, 256), Some(2048));
        assert_eq!(ar_cuda_graph_capacity(2000, 4096), Some(8192));
        assert_eq!(
            ar_cuda_graph_capacity(10_000, 20_000),
            Some(DECODE_ROPE_CACHE_LEN)
        );
        assert_eq!(ar_cuda_graph_capacity(100, 0), None);
        assert_eq!(ar_cuda_graph_capacity(DECODE_ROPE_CACHE_LEN, 1), None);
    }
}
