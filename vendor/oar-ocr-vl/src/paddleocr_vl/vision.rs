use super::config::PaddleOcrVlVisionConfig;
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing, rotate_half};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::Module;
use rayon::prelude::*;

/// Above this seq length vision attention runs chunked along the query dim
/// instead of materializing the full `[seq, seq]` buffer. Kept above v1.5's
/// worst-case seq (≈5040) because chunking shifts cuBLAS tiling and a few
/// low-confidence argmax tokens; the full path keeps v1.5 byte-stable.
const ATTN_FULL_SEQ_THRESHOLD: usize = 8192;
/// Query chunk size for chunked attention (~110 MB F32 softmax scratch at
/// seq_k = 3600, heads = 16).
const ATTN_CHUNK_SIZE: usize = 512;
/// Free-memory headroom for allocations after the softmax scratch peak.
const ATTN_FREE_MEM_HEADROOM: usize = 256 * 1024 * 1024;

/// [`ATTN_FULL_SEQ_THRESHOLD`], overridable via the
/// `OAR_VL_ATTN_FULL_SEQ_THRESHOLD` env var (`0` forces chunked attention).
fn attn_full_seq_threshold() -> usize {
    static THRESHOLD: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        match std::env::var("OAR_VL_ATTN_FULL_SEQ_THRESHOLD") {
            Ok(v) => match v.trim().parse() {
                Ok(t) => {
                    tracing::info!(
                        "vision attention full-seq threshold {t} from OAR_VL_ATTN_FULL_SEQ_THRESHOLD"
                    );
                    t
                }
                Err(_) => {
                    tracing::warn!(
                        "ignoring invalid OAR_VL_ATTN_FULL_SEQ_THRESHOLD value '{v}' (expected an integer)"
                    );
                    ATTN_FULL_SEQ_THRESHOLD
                }
            },
            Err(_) => ATTN_FULL_SEQ_THRESHOLD,
        }
    })
}

/// Peak transient bytes of the full-matrix path: the `[b, heads, seq, seq]`
/// scores in the compute dtype, their F32 copy, the F32 softmax output, and
/// the cast back are all live at once. `None` on `usize` overflow.
fn eager_attn_scratch_bytes(b: usize, heads: usize, seq: usize, dtype: DType) -> Option<usize> {
    let per_element = 2 * dtype.size_in_bytes() + 2 * DType::F32.size_in_bytes();
    b.checked_mul(heads)?
        .checked_mul(seq)?
        .checked_mul(seq)?
        .checked_mul(per_element)
}

/// Whether the full-matrix path fits in free device memory. Unknown free
/// memory (CPU, query failure) keeps the full path; an overflowed scratch
/// estimate (`None`) never fits.
fn eager_attn_fits(device: &Device, scratch_bytes: Option<usize>) -> bool {
    let Some(scratch_bytes) = scratch_bytes else {
        return false;
    };
    match crate::utils::free_device_memory(device) {
        Some(free) => match scratch_bytes.checked_add(ATTN_FREE_MEM_HEADROOM) {
            Some(needed) if needed <= free => true,
            _ => {
                tracing::debug!(
                    "vision attention falling back to chunked path: needs ~{} MiB scratch \
                     but only {} MiB device memory is free",
                    scratch_bytes / (1024 * 1024),
                    free / (1024 * 1024),
                );
                false
            }
        },
        None => true,
    }
}

/// SigLIP-style 2D rotary embedding for vision encoder
#[derive(Debug, Clone)]
struct SigLIPRotaryEmbedding {
    inv_freq: Tensor,
}

impl SigLIPRotaryEmbedding {
    fn new(dim: usize, theta: f64, device: &Device) -> Result<Self, Error> {
        let inv_freq = crate::utils::vision_inv_freq(dim, theta, "PaddleOCR-VL", device)?;
        Ok(Self { inv_freq })
    }

    /// Compute freqs for positions 0..seqlen
    fn forward(&self, seqlen: usize, device: &Device) -> Result<Tensor, Error> {
        let seq: Vec<f32> = (0..seqlen).map(|i| i as f32).collect();
        let seq = Tensor::from_vec(seq, (seqlen,), device).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision rope seq tensor failed",
                e,
            )
        })?;
        // outer product: (seqlen, dim/2)
        let inv_freq = self.inv_freq.to_dtype(DType::F32).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision inv_freq cast failed",
                e,
            )
        })?;
        // seq: (seqlen,), inv_freq: (dim/2,) -> freqs: (seqlen, dim/2)
        let freqs = seq
            .unsqueeze(1)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision seq unsqueeze failed",
                    e,
                )
            })?
            .broadcast_mul(&inv_freq.unsqueeze(0).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision inv_freq unsqueeze failed",
                    e,
                )
            })?)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision rope outer product failed",
                    e,
                )
            })?;
        Ok(freqs)
    }
}

fn rotate_half_vision(x: &Tensor) -> Result<Tensor, Error> {
    rotate_half(x)
}

/// Apply rotary position embedding for vision (2D)
fn apply_rotary_pos_emb_vision(
    q: &Tensor,
    k: &Tensor,
    cos: &Tensor,
    sin: &Tensor,
) -> Result<(Tensor, Tensor), Error> {
    // q, k: (batch, seq, num_heads, head_dim)
    // cos, sin: (seq, head_dim) -> need to unsqueeze for broadcasting
    let orig_dtype = q.dtype();

    let q = q.to_dtype(DType::F32).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "PaddleOCR-VL: vision rope q cast failed",
            e,
        )
    })?;
    let k = k.to_dtype(DType::F32).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "PaddleOCR-VL: vision rope k cast failed",
            e,
        )
    })?;

    // cos, sin: (seq, head_dim) -> (1, seq, 1, head_dim)
    let cos = cos
        .unsqueeze(0)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision cos unsqueeze0 failed",
                e,
            )
        })?
        .unsqueeze(2)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision cos unsqueeze2 failed",
                e,
            )
        })?
        .to_dtype(DType::F32)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision cos cast failed",
                e,
            )
        })?;

    let sin = sin
        .unsqueeze(0)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision sin unsqueeze0 failed",
                e,
            )
        })?
        .unsqueeze(2)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision sin unsqueeze2 failed",
                e,
            )
        })?
        .to_dtype(DType::F32)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision sin cast failed",
                e,
            )
        })?;

    // q_embed = q * cos + rotate_half(q) * sin
    let q_rot = rotate_half_vision(&q)?;
    let q_embed = q
        .broadcast_mul(&cos)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision q*cos failed",
                e,
            )
        })?
        .broadcast_add(&q_rot.broadcast_mul(&sin).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision rotate_half(q)*sin failed",
                e,
            )
        })?)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision q rope add failed",
                e,
            )
        })?;

    let k_rot = rotate_half_vision(&k)?;
    let k_embed = k
        .broadcast_mul(&cos)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision k*cos failed",
                e,
            )
        })?
        .broadcast_add(&k_rot.broadcast_mul(&sin).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision rotate_half(k)*sin failed",
                e,
            )
        })?)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision k rope add failed",
                e,
            )
        })?;

    let q_embed = q_embed.to_dtype(orig_dtype).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "PaddleOCR-VL: vision q_embed cast back failed",
            e,
        )
    })?;
    let k_embed = k_embed.to_dtype(orig_dtype).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "PaddleOCR-VL: vision k_embed cast back failed",
            e,
        )
    })?;

    Ok((q_embed, k_embed))
}

#[derive(Debug, Clone)]
struct VisionAttention {
    q_proj: candle_nn::Linear,
    k_proj: candle_nn::Linear,
    v_proj: candle_nn::Linear,
    out_proj: candle_nn::Linear,
    num_heads: usize,
    head_dim: usize,
    scale: f64,
}

impl VisionAttention {
    fn load(cfg: &PaddleOcrVlVisionConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let q_proj = candle_nn::linear(cfg.hidden_size, cfg.hidden_size, vb.pp("q_proj"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision q_proj", e))?;
        let k_proj = candle_nn::linear(cfg.hidden_size, cfg.hidden_size, vb.pp("k_proj"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision k_proj", e))?;
        let v_proj = candle_nn::linear(cfg.hidden_size, cfg.hidden_size, vb.pp("v_proj"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision v_proj", e))?;
        let out_proj = candle_nn::linear(cfg.hidden_size, cfg.hidden_size, vb.pp("out_proj"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision out_proj", e))?;
        if !cfg.hidden_size.is_multiple_of(cfg.num_attention_heads) {
            return Err(Error::Config {
                message: format!(
                    "PaddleOCR-VL vision: hidden_size ({}) must be divisible by num_attention_heads ({})",
                    cfg.hidden_size, cfg.num_attention_heads
                ),
            });
        }
        let head_dim = cfg.hidden_size / cfg.num_attention_heads;
        Ok(Self {
            q_proj,
            k_proj,
            v_proj,
            out_proj,
            num_heads: cfg.num_attention_heads,
            head_dim,
            scale: (head_dim as f64).powf(-0.5),
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        rope_emb: Option<(&Tensor, &Tensor)>,
    ) -> Result<Tensor, Error> {
        let (b, seq, embed_dim) = hidden_states
            .dims3()
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision attn dims3", e))?;

        let q = self
            .q_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision q_proj", e))?
            .reshape((b, seq, self.num_heads, self.head_dim))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision q reshape", e))?;

        let k = self
            .k_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision k_proj", e))?
            .reshape((b, seq, self.num_heads, self.head_dim))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision k reshape", e))?;

        let v = self
            .v_proj
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision v_proj", e))?
            .reshape((b, seq, self.num_heads, self.head_dim))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision v reshape", e))?
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision v transpose", e))?
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision v contiguous", e))?;

        // Apply rotary embeddings if provided
        let (q, k) = if let Some((cos, sin)) = rope_emb {
            let (q_rot, k_rot) = apply_rotary_pos_emb_vision(&q, &k, cos, sin)?;
            (
                q_rot
                    .transpose(1, 2)
                    .map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision q_rot transpose", e)
                    })?
                    .contiguous()
                    .map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision q_rot contiguous", e)
                    })?,
                k_rot
                    .transpose(1, 2)
                    .map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision k_rot transpose", e)
                    })?
                    .contiguous()
                    .map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision k_rot contiguous", e)
                    })?,
            )
        } else {
            (
                q.transpose(1, 2)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision q transpose", e))?
                    .contiguous()
                    .map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision q contiguous", e)
                    })?,
                k.transpose(1, 2)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision k transpose", e))?
                    .contiguous()
                    .map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision k contiguous", e)
                    })?,
            )
        };

        // Metal's fused SDPA supports head_dim 72 and replaces the materialized
        // `[b, heads, seq, seq]` scores and their F32 softmax with one dispatch.
        // Metal-only: the eager path stays byte-stable for CUDA. Still gated on
        // the threshold so `OAR_VL_ATTN_FULL_SEQ_THRESHOLD` keeps forcing the
        // chunked path as documented, even though the fused kernel is the
        // leanest of the three.
        if hidden_states.device().is_metal()
            && seq <= attn_full_seq_threshold()
            && let Some(fused) =
                crate::attention::try_fused_sdpa(&q, &k, &v, None, self.scale, false)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision fused sdpa", e))?
        {
            let attn_output = fused
                .transpose(1, 2)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision out transpose", e))?
                .reshape((b, seq, embed_dim))
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision out reshape", e))?;
            return self
                .out_proj
                .forward(&attn_output)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision out_proj", e));
        }

        let kt = k
            .transpose(2, 3)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision k t23", e))?
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision k t23 contiguous", e))?;

        // Full-matrix path only when seq is below the threshold AND its softmax
        // scratch fits in free device memory — large images otherwise OOM on
        // small-VRAM GPUs (issue #147) and thrash the unified-memory working set
        // on small Macs (issue #177). The chunked fallback is numerically
        // equivalent (mirrors PyTorch SDPA's memory profile).
        let attn_output = if seq <= attn_full_seq_threshold()
            && eager_attn_fits(
                hidden_states.device(),
                eager_attn_scratch_bytes(b, self.num_heads, seq, q.dtype()),
            ) {
            // Kept byte-identical to the original implementation: removing the
            // final `.contiguous()` changes cuBLAS' matmul-shape selection and
            // shifts low-confidence argmax tokens.
            let scores = q
                .matmul(&kt)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision qk matmul", e))?
                .affine(self.scale, 0.0)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision scaling", e))?;
            let probs =
                candle_nn::ops::softmax_last_dim(&scores.to_dtype(DType::F32).map_err(|e| {
                    candle_to_ocr_inference("PaddleOCR-VL", "vision attn cast f32", e)
                })?)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision attn softmax", e))?
                .to_dtype(v.dtype())
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision attn cast back", e))?
                .contiguous()
                .map_err(|e| {
                    candle_to_ocr_inference("PaddleOCR-VL", "vision attn contiguous", e)
                })?;
            probs
                .matmul(&v)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision av matmul", e))?
        } else {
            let mut chunks: Vec<Tensor> = Vec::with_capacity(seq.div_ceil(ATTN_CHUNK_SIZE));
            let mut start = 0;
            while start < seq {
                let len = ATTN_CHUNK_SIZE.min(seq - start);
                let q_chunk = q
                    .narrow(2, start, len)
                    .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision q narrow", e))?;
                let scores = q_chunk
                    .matmul(&kt)
                    .map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision qk matmul (chunk)", e)
                    })?
                    .affine(self.scale, 0.0)
                    .map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision scaling (chunk)", e)
                    })?;
                let probs = candle_nn::ops::softmax_last_dim(
                    &scores.to_dtype(DType::F32).map_err(|e| {
                        candle_to_ocr_inference("PaddleOCR-VL", "vision attn cast f32 (chunk)", e)
                    })?,
                )
                .map_err(|e| {
                    candle_to_ocr_inference("PaddleOCR-VL", "vision attn softmax (chunk)", e)
                })?
                .to_dtype(v.dtype())
                .map_err(|e| {
                    candle_to_ocr_inference("PaddleOCR-VL", "vision attn cast back (chunk)", e)
                })?;
                let out_chunk = probs.matmul(&v).map_err(|e| {
                    candle_to_ocr_inference("PaddleOCR-VL", "vision av matmul (chunk)", e)
                })?;
                chunks.push(out_chunk);
                start += len;
            }
            Tensor::cat(&chunks, 2)
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision av concat", e))?
        };

        let attn_output = attn_output
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision out transpose", e))?
            .reshape((b, seq, embed_dim))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision out reshape", e))?;

        self.out_proj
            .forward(&attn_output)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision out_proj", e))
    }
}

#[derive(Debug, Clone)]
struct VisionMlp {
    fc1: candle_nn::Linear,
    fc2: candle_nn::Linear,
    act: candle_nn::Activation,
}

impl VisionMlp {
    fn load(cfg: &PaddleOcrVlVisionConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let fc1 = candle_nn::linear(cfg.hidden_size, cfg.intermediate_size, vb.pp("fc1"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision fc1", e))?;
        let fc2 = candle_nn::linear(cfg.intermediate_size, cfg.hidden_size, vb.pp("fc2"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision fc2", e))?;
        let act = serde_json::from_str::<candle_nn::Activation>(&format!("\"{}\"", cfg.hidden_act))
            .unwrap_or(candle_nn::Activation::GeluPytorchTanh);
        Ok(Self { fc1, fc2, act })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor, Error> {
        let xs = self
            .fc1
            .forward(xs)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision mlp fc1", e))?;
        let xs = self
            .act
            .forward(&xs)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision mlp act", e))?;
        self.fc2
            .forward(&xs)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision mlp fc2", e))
    }
}

#[derive(Debug, Clone)]
struct VisionEncoderLayer {
    layer_norm1: candle_nn::LayerNorm,
    self_attn: VisionAttention,
    layer_norm2: candle_nn::LayerNorm,
    mlp: VisionMlp,
}

impl VisionEncoderLayer {
    fn load(cfg: &PaddleOcrVlVisionConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let ln_cfg = candle_nn::LayerNormConfig {
            eps: cfg.layer_norm_eps,
            remove_mean: true,
            affine: true,
        };
        let layer_norm1 = candle_nn::layer_norm(cfg.hidden_size, ln_cfg, vb.pp("layer_norm1"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision layer_norm1", e))?;
        let self_attn = VisionAttention::load(cfg, vb.pp("self_attn"))?;
        let layer_norm2 = candle_nn::layer_norm(cfg.hidden_size, ln_cfg, vb.pp("layer_norm2"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision layer_norm2", e))?;
        let mlp = VisionMlp::load(cfg, vb.pp("mlp"))?;
        Ok(Self {
            layer_norm1,
            self_attn,
            layer_norm2,
            mlp,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        rope_emb: Option<(&Tensor, &Tensor)>,
    ) -> Result<Tensor, Error> {
        let residual = hidden_states.clone();
        let hidden_states = self
            .layer_norm1
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision ln1", e))?;
        let attn = self.self_attn.forward(&hidden_states, rope_emb)?;
        let hidden_states = (&residual + &attn)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision attn residual add", e))?;

        let residual = hidden_states.clone();
        let hidden_states = self
            .layer_norm2
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision ln2", e))?;
        let mlp = self.mlp.forward(&hidden_states)?;
        (&residual + &mlp)
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "vision mlp residual add", e))
    }
}

#[derive(Debug, Clone)]
struct VisionEmbeddings {
    patch_embedding: candle_nn::Conv2d,
    position_embedding: candle_nn::Embedding,
}

impl VisionEmbeddings {
    fn load(cfg: &PaddleOcrVlVisionConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let conv_cfg = candle_nn::Conv2dConfig {
            padding: 0,
            stride: cfg.patch_size,
            dilation: 1,
            groups: 1,
            cudnn_fwd_algo: None,
        };
        let patch_embedding = candle_nn::conv2d(
            cfg.num_channels,
            cfg.hidden_size,
            cfg.patch_size,
            conv_cfg,
            vb.pp("patch_embedding"),
        )
        .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load vision patch_embedding", e))?;
        let grid = cfg.image_size / cfg.patch_size;
        let num_positions = grid * grid;
        let position_embedding =
            candle_nn::embedding(num_positions, cfg.hidden_size, vb.pp("position_embedding"))
                .map_err(|e| {
                    candle_to_ocr_inference("PaddleOCR-VL", "load vision position_embedding", e)
                })?;
        Ok(Self {
            patch_embedding,
            position_embedding,
        })
    }

    fn interpolate_pos_encoding(
        &self,
        height: usize,
        width: usize,
        device: &Device,
        dtype: DType,
    ) -> Result<Tensor, Error> {
        if height == 0 || width == 0 {
            return Err(Error::InvalidInput {
                message: format!(
                    "PaddleOCR-VL: vision interpolate_pos_encoding requires height/width > 0, got {height}x{width}"
                ),
            });
        }

        let pos_w = self.position_embedding.embeddings();
        let (num_positions, dim) = pos_w.dims2().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision position_embedding dims2 failed",
                e,
            )
        })?;

        let grid = (num_positions as f64).sqrt() as usize;
        if grid * grid != num_positions {
            return Err(Error::Config {
                message: format!(
                    "PaddleOCR-VL: vision position_embedding weight is not a square grid, \
                     got num_positions={num_positions} (nearest square is {grid}×{grid}={})",
                    grid * grid
                ),
            });
        }

        // Match PyTorch's `interpolate(..., mode=\"bilinear\", align_corners=False)`.
        // Python code:
        //   patch_pos_embed = position_embedding.weight.unsqueeze(0)
        //   patch_pos_embed = patch_pos_embed.reshape(1, grid, grid, dim).permute(0, 3, 1, 2)
        //   patch_pos_embed = F.interpolate(..., size=(height, width), mode=\"bilinear\", align_corners=False)
        //   patch_pos_embed = patch_pos_embed.permute(0, 2, 3, 1).view(1, -1, dim)
        let base = pos_w
            .to_dtype(DType::F32)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision position_embedding cast to f32 failed",
                    e,
                )
            })?
            .flatten_all()
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision position_embedding flatten failed",
                    e,
                )
            })?
            .to_vec1::<f32>()
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision position_embedding to_vec failed",
                    e,
                )
            })?;

        let out = interpolate_bilinear_align_corners_false(&base, grid, grid, height, width, dim);
        Tensor::from_vec(out, (height * width, dim), device)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision interpolated pos embedding tensor failed",
                    e,
                )
            })?
            .to_dtype(dtype)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision interpolated pos embedding cast failed",
                    e,
                )
            })
    }
}

#[derive(Debug, Clone)]
pub struct VisionModel {
    embeddings: VisionEmbeddings,
    layers: Vec<VisionEncoderLayer>,
    post_layernorm: candle_nn::LayerNorm,
    rotary_pos_emb: SigLIPRotaryEmbedding,
}

impl VisionModel {
    pub fn load(cfg: &PaddleOcrVlVisionConfig, vb: candle_nn::VarBuilder) -> Result<Self, Error> {
        let embeddings = VisionEmbeddings::load(cfg, vb.pp("embeddings"))?;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            layers.push(VisionEncoderLayer::load(
                cfg,
                vb.pp(format!("encoder.layers.{i}")),
            )?);
        }
        let ln_cfg = candle_nn::LayerNormConfig {
            eps: cfg.layer_norm_eps,
            remove_mean: true,
            affine: true,
        };
        let post_layernorm =
            candle_nn::layer_norm(cfg.hidden_size, ln_cfg, vb.pp("post_layernorm")).map_err(
                |e| candle_to_ocr_inference("PaddleOCR-VL", "load vision post_layernorm", e),
            )?;

        // Create rotary embedding for vision
        // Python: SigLIPRotaryEmbedding(head_dim // 2)
        let head_dim = cfg.hidden_size / cfg.num_attention_heads;
        let rotary_pos_emb = SigLIPRotaryEmbedding::new(head_dim / 2, 10000.0, vb.device())?;

        Ok(Self {
            embeddings,
            layers,
            post_layernorm,
            rotary_pos_emb,
        })
    }

    pub fn forward(
        &self,
        pixel_values: &Tensor,
        image_grid_thw: &[(usize, usize, usize)],
    ) -> Result<Vec<Tensor>, Error> {
        // One image at a time: the body below packs all patches into a single
        // unmasked sequence, so with several images every patch of one attends
        // to the others and the features blend (upstream separates them with a
        // block-diagonal `cu_seqlens` mask). Per-image encoding computes the
        // same thing and leaves the single-image path byte-identical.
        if image_grid_thw.len() > 1 {
            let mut outputs = Vec::with_capacity(image_grid_thw.len());
            let mut start = 0usize;
            for &grid in image_grid_thw {
                let (t, h, w) = grid;
                let len = t * h * w;
                let patches = pixel_values.narrow(0, start, len).map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "PaddleOCR-VL: vision slice pixel_values per image failed",
                        e,
                    )
                })?;
                outputs.extend(self.forward(&patches, std::slice::from_ref(&grid))?);
                start += len;
            }
            return Ok(outputs);
        }

        let device = pixel_values.device();

        // Compute height/width position IDs for 2D rope
        let mut height_position_ids: Vec<i64> = Vec::new();
        let mut width_position_ids: Vec<i64> = Vec::new();

        for &(t, h, w) in image_grid_thw {
            let hw = (h * w) as u32;
            let numel = t * h * w;
            for idx in 0..numel as u32 {
                // height_id = patch_idx // w, width_id = patch_idx % w
                let patch_idx = idx % hw;
                height_position_ids.push((patch_idx / w as u32) as i64);
                width_position_ids.push((patch_idx % w as u32) as i64);
            }
        }

        // Build rope embeddings
        // pids = stack([height_ids, width_ids], dim=-1)  -> (num_patches, 2)
        // rope_emb_max_grid = rotary_pos_emb(max_grid_size) -> (max_grid_size, dim/2)
        // rope_emb = rope_emb_max_grid[pids].flatten(1).repeat(1, 2) -> (num_patches, head_dim)
        let max_h = height_position_ids.iter().copied().max().unwrap_or(0) as usize + 1;
        let max_w = width_position_ids.iter().copied().max().unwrap_or(0) as usize + 1;
        let max_grid_size = max_h.max(max_w);

        // Compute freqs for all positions 0..max_grid_size
        let freqs = self.rotary_pos_emb.forward(max_grid_size, device)?;

        // Gather freqs for height and width positions
        let h_ids = Tensor::new(height_position_ids.clone(), device)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision height_ids tensor failed",
                    e,
                )
            })?
            .to_dtype(DType::U32)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision height_ids cast failed",
                    e,
                )
            })?;

        let w_ids = Tensor::new(width_position_ids, device)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision width_ids tensor failed",
                    e,
                )
            })?
            .to_dtype(DType::U32)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision width_ids cast failed",
                    e,
                )
            })?;

        // freqs_h = freqs[height_ids], freqs_w = freqs[width_ids]
        let freqs_h = freqs.index_select(&h_ids, 0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision freqs_h gather failed",
                e,
            )
        })?;
        let freqs_w = freqs.index_select(&w_ids, 0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision freqs_w gather failed",
                e,
            )
        })?;

        // Concatenate to get (num_patches, head_dim/2) then repeat to get (num_patches, head_dim)
        let rope_emb = Tensor::cat(&[&freqs_h, &freqs_w], 1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision rope_emb cat failed",
                e,
            )
        })?;
        // Repeat to double the dimension for full head_dim
        let rope_emb = Tensor::cat(&[&rope_emb, &rope_emb], 1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision rope_emb repeat failed",
                e,
            )
        })?;

        let cos = rope_emb.cos().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision rope cos failed",
                e,
            )
        })?;
        let sin = rope_emb.sin().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision rope sin failed",
                e,
            )
        })?;

        let patch = self
            .embeddings
            .patch_embedding
            .forward(pixel_values)
            .map_err(|e| {
                candle_to_ocr_inference("PaddleOCR-VL", "vision patch_embedding forward", e)
            })?;

        let patch = patch
            .flatten_from(2)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision patch flatten failed",
                    e,
                )
            })?
            .squeeze(2)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision patch squeeze failed",
                    e,
                )
            })?;

        // Upstream adds *interpolated* 2D position embeddings (not `packing_position_embedding`)
        // when `interpolate_pos_encoding=True`.
        let mut segments: Vec<Tensor> = Vec::with_capacity(image_grid_thw.len());
        let mut start = 0usize;
        for &(t, h, w) in image_grid_thw {
            if t != 1 {
                return Err(Error::InvalidInput {
                    message: "PaddleOCR-VL: vision temporal inputs are not supported (t != 1)"
                        .to_string(),
                });
            }
            let len = t * h * w;
            let end = start + len;
            let seg = patch.i((start..end, ..)).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision slice patch embeddings failed",
                    e,
                )
            })?;
            let pos = self
                .embeddings
                .interpolate_pos_encoding(h, w, device, seg.dtype())?;
            let seg = seg.broadcast_add(&pos).map_err(|e| {
                candle_to_ocr_inference("PaddleOCR-VL", "vision add interpolated pos", e)
            })?;
            segments.push(seg);
            start = end;
        }

        let refs: Vec<&Tensor> = segments.iter().collect();
        let patch = Tensor::cat(&refs, 0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision concat pos-added patches failed",
                e,
            )
        })?;

        let mut hidden = patch.unsqueeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision add batch dim failed",
                e,
            )
        })?;

        // Pass rope embeddings to each layer
        let rope_emb = Some((&cos, &sin));
        for layer in self.layers.iter() {
            hidden = layer.forward(&hidden, rope_emb)?;
        }

        hidden = self.post_layernorm.forward(&hidden).map_err(|e| {
            candle_to_ocr_inference("PaddleOCR-VL", "vision post_layernorm forward", e)
        })?;

        let hidden = hidden.squeeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: vision squeeze batch failed",
                e,
            )
        })?;

        let mut out = Vec::with_capacity(image_grid_thw.len());
        let mut start = 0usize;
        for &(t, h, w) in image_grid_thw {
            let len = t * h * w;
            let end = start + len;
            let slice = hidden.i((start..end, ..)).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: vision slice failed",
                    e,
                )
            })?;
            out.push(slice);
            start = end;
        }
        Ok(out)
    }
}

fn interpolate_bilinear_align_corners_false(
    base: &[f32],
    in_h: usize,
    in_w: usize,
    out_h: usize,
    out_w: usize,
    dim: usize,
) -> Vec<f32> {
    debug_assert_eq!(base.len(), in_h * in_w * dim);

    let mut y_lut = Vec::with_capacity(out_h);
    for oy in 0..out_h {
        let in_y = ((oy as f32) + 0.5) * (in_h as f32) / (out_h as f32) - 0.5;
        let in_y = in_y.clamp(0.0, (in_h - 1) as f32);
        let y0 = in_y.floor() as usize;
        let y1 = (y0 + 1).min(in_h - 1);
        let wy1 = in_y - (y0 as f32);
        let wy0 = 1.0 - wy1;
        y_lut.push((y0, y1, wy0, wy1));
    }

    let mut x_lut = Vec::with_capacity(out_w);
    for ox in 0..out_w {
        let in_x = ((ox as f32) + 0.5) * (in_w as f32) / (out_w as f32) - 0.5;
        let in_x = in_x.clamp(0.0, (in_w - 1) as f32);
        let x0 = in_x.floor() as usize;
        let x1 = (x0 + 1).min(in_w - 1);
        let wx1 = in_x - (x0 as f32);
        let wx0 = 1.0 - wx1;
        x_lut.push((x0, x1, wx0, wx1));
    }

    let mut out = vec![0f32; out_h * out_w * dim];
    out.par_chunks_mut(dim)
        .enumerate()
        .for_each(|(idx, chunk)| {
            let oy = idx / out_w;
            let ox = idx % out_w;

            let (y0, y1, wy0, wy1) = y_lut[oy];
            let (x0, x1, wx0, wx1) = x_lut[ox];
            let w00 = wy0 * wx0;
            let w01 = wy0 * wx1;
            let w10 = wy1 * wx0;
            let w11 = wy1 * wx1;

            let base00 = (y0 * in_w + x0) * dim;
            let base01 = (y0 * in_w + x1) * dim;
            let base10 = (y1 * in_w + x0) * dim;
            let base11 = (y1 * in_w + x1) * dim;

            for d in 0..dim {
                chunk[d] = base[base00 + d] * w00
                    + base[base01 + d] * w01
                    + base[base10 + d] * w10
                    + base[base11 + d] * w11;
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::interpolate_bilinear_align_corners_false;

    #[test]
    fn eager_attn_scratch_bytes_estimate_and_overflow() {
        use candle_core::DType;
        // f16 scores + f32 copy + f32 softmax output + f16 cast back = 12 B/elem.
        assert_eq!(
            super::eager_attn_scratch_bytes(1, 16, 5000, DType::F16),
            Some(16 * 5000 * 5000 * 12)
        );
        // An overflowing product must report "doesn't fit", not wrap around.
        assert_eq!(
            super::eager_attn_scratch_bytes(1, 16, usize::MAX / 2, DType::F16),
            None
        );
    }

    #[test]
    fn interpolate_same_size_is_identity() {
        // 2x2 grid, dim=1:
        // [[0, 1],
        //  [2, 3]]
        let base = vec![0f32, 1f32, 2f32, 3f32];
        let out = interpolate_bilinear_align_corners_false(&base, 2, 2, 2, 2, 1);
        assert_eq!(out, base);
    }

    #[test]
    fn interpolate_to_1x1_matches_pytorch_align_corners_false_center() {
        // With align_corners=False, a 1x1 output samples the center of the input grid.
        // For a 2x2 grid, that's the bilinear average of all 4 corners.
        let base = vec![0f32, 1f32, 2f32, 3f32];
        let out = interpolate_bilinear_align_corners_false(&base, 2, 2, 1, 1, 1);
        assert_eq!(out.len(), 1);
        assert!((out[0] - 1.5).abs() < 1e-6, "got {}", out[0]);
    }
}
