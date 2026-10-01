use super::config::GlmOcrVisionConfig;
use crate::attention::on_compute_device;
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{D, DType, Device, IndexOp, Tensor};
use candle_nn::{
    Conv2d, Conv2dConfig, Linear, Module, RmsNorm, VarBuilder, linear_no_bias, rms_norm,
};

#[derive(Debug, Clone)]
struct GlmOcrVisionPatchEmbed {
    weight: Tensor,
    bias: Tensor,
}

impl GlmOcrVisionPatchEmbed {
    fn load(cfg: &GlmOcrVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let vb = vb.pp("patch_embed").pp("proj");
        let weight = vb
            .get(
                (
                    cfg.hidden_size,
                    cfg.in_channels,
                    cfg.temporal_patch_size,
                    cfg.patch_size,
                    cfg.patch_size,
                ),
                "weight",
            )
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "load patch_embed.weight", e))?;
        let bias = vb
            .get(cfg.hidden_size, "bias")
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "load patch_embed.bias", e))?;

        let in_features =
            cfg.in_channels * cfg.temporal_patch_size * cfg.patch_size * cfg.patch_size;
        let weight = weight
            .reshape((cfg.hidden_size, in_features))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: reshape patch_embed weight",
                    e,
                )
            })?;

        Ok(Self { weight, bias })
    }

    fn forward(&self, hidden_states: &Tensor) -> Result<Tensor, Error> {
        let w_t = self.weight.transpose(0, 1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: patch_embed weight transpose",
                e,
            )
        })?;
        let out = hidden_states.matmul(&w_t).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: patch_embed matmul",
                e,
            )
        })?;
        out.broadcast_add(&self.bias).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: patch_embed bias add",
                e,
            )
        })
    }
}

#[derive(Debug, Clone)]
struct GlmOcrVisionRotaryEmbedding {
    inv_freq: Tensor,
}

impl GlmOcrVisionRotaryEmbedding {
    fn new(dim: usize, device: &Device, dtype: DType) -> Result<Self, Error> {
        // Shared with MinerU2.5 / PaddleOCR-VL: `1 / theta^(i/dim)` computed in
        // f64 then narrowed to f32 (theta = 10000). The subsequent dtype cast
        // dominates any f64-vs-f32 difference at the model's working precision.
        let inv_freq = crate::utils::vision_inv_freq(dim, 10000.0, "GLM-OCR", device)?
            .to_dtype(dtype)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: cast vision inv_freq",
                    e,
                )
            })?;
        Ok(Self { inv_freq })
    }

    fn forward(&self, seqlen: usize) -> Result<Tensor, Error> {
        let device = self.inv_freq.device();
        let dtype = self.inv_freq.dtype();

        let inv_len = self.inv_freq.dims1().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision inv_freq dims1",
                e,
            )
        })?;

        // Use on_compute_device to handle Metal's lack of support for arange
        on_compute_device(device, |compute_device| {
            let seq = Tensor::arange(0u32, seqlen as u32, compute_device)?
                .to_dtype(dtype)?
                .reshape((seqlen, 1))?;

            let inv = self
                .inv_freq
                .to_device(compute_device)?
                .reshape((1, inv_len))?;

            seq.matmul(&inv)
        })
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision rope forward failed",
                e,
            )
        })
    }
}

fn apply_rotary_pos_emb_vision(
    q: &Tensor,
    k: &Tensor,
    cos: &Tensor,
    sin: &Tensor,
) -> Result<(Tensor, Tensor), Error> {
    let cos = cos.unsqueeze(D::Minus2).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: vision cos unsqueeze",
            e,
        )
    })?;
    let sin = sin.unsqueeze(D::Minus2).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: vision sin unsqueeze",
            e,
        )
    })?;

    let q_rot = crate::utils::rotate_half(q)?;
    let k_rot = crate::utils::rotate_half(k)?;

    let q_mul = q.broadcast_mul(&cos).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: vision q*cos",
            e,
        )
    })?;
    let q_rot_mul = q_rot.broadcast_mul(&sin).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: vision rotate_half(q)*sin",
            e,
        )
    })?;
    let q_out = (&q_mul + &q_rot_mul).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: vision q apply rope",
            e,
        )
    })?;

    let k_mul = k.broadcast_mul(&cos).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: vision k*cos",
            e,
        )
    })?;
    let k_rot_mul = k_rot.broadcast_mul(&sin).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: vision rotate_half(k)*sin",
            e,
        )
    })?;
    let k_out = (&k_mul + &k_rot_mul).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "GLM-OCR: vision k apply rope",
            e,
        )
    })?;
    Ok((q_out, k_out))
}

#[derive(Debug, Clone)]
struct GlmOcrVisionAttention {
    qkv: Linear,
    proj: Linear,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    num_heads: usize,
    head_dim: usize,
    scaling: f64,
}

impl GlmOcrVisionAttention {
    fn load(cfg: &GlmOcrVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let qkv = candle_nn::linear_b(
            cfg.hidden_size,
            cfg.hidden_size * 3,
            cfg.attention_bias,
            vb.pp("attn").pp("qkv"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "load vision qkv", e))?;
        let proj = candle_nn::linear_b(
            cfg.hidden_size,
            cfg.hidden_size,
            cfg.attention_bias,
            vb.pp("attn").pp("proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "load vision proj", e))?;

        let head_dim = cfg.hidden_size / cfg.num_heads;
        let q_norm = rms_norm(head_dim, cfg.rms_norm_eps, vb.pp("attn").pp("q_norm"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "load vision q_norm", e))?;
        let k_norm = rms_norm(head_dim, cfg.rms_norm_eps, vb.pp("attn").pp("k_norm"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "load vision k_norm", e))?;

        Ok(Self {
            qkv,
            proj,
            q_norm,
            k_norm,
            num_heads: cfg.num_heads,
            head_dim,
            scaling: 1.0 / (head_dim as f64).sqrt(),
        })
    }

    fn forward(&self, hidden_states: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let seq_len = hidden_states.dim(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision attention seq_len",
                e,
            )
        })?;
        let qkv = self
            .qkv
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision qkv forward", e))?;
        let qkv = qkv
            .reshape((seq_len, 3, self.num_heads, self.head_dim))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision qkv reshape",
                    e,
                )
            })?;

        let q = qkv.i((.., 0, .., ..)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision q slice",
                e,
            )
        })?;
        let k = qkv.i((.., 1, .., ..)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision k slice",
                e,
            )
        })?;
        let v = qkv.i((.., 2, .., ..)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision v slice",
                e,
            )
        })?;

        let q = self
            .q_norm
            .forward(&q)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision q_norm forward", e))?;
        let k = self
            .k_norm
            .forward(&k)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision k_norm forward", e))?;

        let (q, k) = apply_rotary_pos_emb_vision(&q, &k, cos, sin)?;

        let q = q
            .transpose(0, 1)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision q transpose",
                    e,
                )
            })?
            .contiguous()
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision q contiguous",
                    e,
                )
            })?;
        let k = k
            .transpose(0, 1)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision k transpose",
                    e,
                )
            })?
            .contiguous()
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision k contiguous",
                    e,
                )
            })?;
        let v = v
            .transpose(0, 1)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision v transpose",
                    e,
                )
            })?
            .contiguous()
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision v contiguous",
                    e,
                )
            })?;

        let q = q.unsqueeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision q unsqueeze",
                e,
            )
        })?;
        let k = k.unsqueeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision k unsqueeze",
                e,
            )
        })?;
        let v = v.unsqueeze(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision v unsqueeze",
                e,
            )
        })?;

        let attn =
            crate::attention::scaled_dot_product_attention(&q, &k, &v, None, self.scaling, false)
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision attention", e))?;
        let attn = attn
            .transpose(1, 2)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision attn transpose",
                    e,
                )
            })?
            .reshape((seq_len, self.num_heads * self.head_dim))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision attn reshape",
                    e,
                )
            })?;

        self.proj
            .forward(&attn)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision proj forward", e))
    }
}

#[derive(Debug, Clone)]
struct GlmOcrVisionMlp {
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
    act_fn: candle_nn::Activation,
}

impl GlmOcrVisionMlp {
    fn load(cfg: &GlmOcrVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let gate_proj = candle_nn::linear_b(
            cfg.hidden_size,
            cfg.intermediate_size,
            true,
            vb.pp("mlp").pp("gate_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision mlp gate_proj", e))?;
        let up_proj = candle_nn::linear_b(
            cfg.hidden_size,
            cfg.intermediate_size,
            true,
            vb.pp("mlp").pp("up_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision mlp up_proj", e))?;
        let down_proj = candle_nn::linear_b(
            cfg.intermediate_size,
            cfg.hidden_size,
            true,
            vb.pp("mlp").pp("down_proj"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision mlp down_proj", e))?;

        Ok(Self {
            gate_proj,
            up_proj,
            down_proj,
            act_fn: cfg.hidden_act,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor, Error> {
        let gate = self
            .gate_proj
            .forward(xs)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision mlp gate_proj forward", e))?;
        let gate = gate
            .apply(&self.act_fn)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision mlp act", e))?;
        let up = self
            .up_proj
            .forward(xs)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision mlp up_proj forward", e))?;
        let prod = (&gate * &up)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision mlp gate*up", e))?;
        self.down_proj
            .forward(&prod)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision mlp down_proj forward", e))
    }
}

#[derive(Debug, Clone)]
struct GlmOcrVisionBlock {
    norm1: RmsNorm,
    norm2: RmsNorm,
    attn: GlmOcrVisionAttention,
    mlp: GlmOcrVisionMlp,
}

impl GlmOcrVisionBlock {
    fn load_block(cfg: &GlmOcrVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let norm1 = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm1"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision norm1", e))?;
        let norm2 = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm2"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision norm2", e))?;
        let attn = GlmOcrVisionAttention::load(cfg, vb.clone())?;
        let mlp = GlmOcrVisionMlp::load(cfg, vb)?;
        Ok(Self {
            norm1,
            norm2,
            attn,
            mlp,
        })
    }

    fn forward(&self, hidden_states: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let normed = self
            .norm1
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision norm1 forward", e))?;
        let attn_out = self.attn.forward(&normed, cos, sin)?;
        let hidden_states = (hidden_states + attn_out).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision residual add",
                e,
            )
        })?;
        let normed = self
            .norm2
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision norm2 forward", e))?;
        let mlp_out = self.mlp.forward(&normed)?;
        (hidden_states + mlp_out).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision residual add mlp",
                e,
            )
        })
    }
}

#[derive(Debug, Clone)]
struct GlmOcrVisionPatchMerger {
    proj: Linear,
    post_projection_norm: candle_nn::LayerNorm,
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
    act_fn: candle_nn::Activation,
}

impl GlmOcrVisionPatchMerger {
    fn load(cfg: &GlmOcrVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let proj = linear_no_bias(cfg.out_hidden_size, cfg.out_hidden_size, vb.pp("proj"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger proj", e))?;
        let ln_cfg = candle_nn::LayerNormConfig {
            eps: cfg.rms_norm_eps,
            remove_mean: true,
            affine: true,
        };
        let post_projection_norm =
            candle_nn::layer_norm(cfg.out_hidden_size, ln_cfg, vb.pp("post_projection_norm"))
                .map_err(|e| {
                    candle_to_ocr_inference("GLM-OCR", "vision merger post_projection_norm", e)
                })?;

        let context_dim = cfg.out_hidden_size * cfg.in_channels;
        let gate_proj = linear_no_bias(cfg.out_hidden_size, context_dim, vb.pp("gate_proj"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger gate_proj", e))?;
        let up_proj = linear_no_bias(cfg.out_hidden_size, context_dim, vb.pp("up_proj"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger up_proj", e))?;
        let down_proj = linear_no_bias(context_dim, cfg.out_hidden_size, vb.pp("down_proj"))
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger down_proj", e))?;

        Ok(Self {
            proj,
            post_projection_norm,
            gate_proj,
            up_proj,
            down_proj,
            act_fn: cfg.hidden_act,
        })
    }

    fn forward(&self, hidden_state: &Tensor) -> Result<Tensor, Error> {
        let hidden_state = self
            .proj
            .forward(hidden_state)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger proj forward", e))?;
        let hidden_state = self
            .post_projection_norm
            .forward(&hidden_state)
            .map_err(|e| {
                candle_to_ocr_inference("GLM-OCR", "vision merger post_projection_norm forward", e)
            })?;
        let hidden_state = hidden_state
            .gelu()
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger gelu", e))?;

        let gate = self.gate_proj.forward(&hidden_state).map_err(|e| {
            candle_to_ocr_inference("GLM-OCR", "vision merger gate_proj forward", e)
        })?;
        let gate = gate
            .apply(&self.act_fn)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger act", e))?;
        let up = self
            .up_proj
            .forward(&hidden_state)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger up_proj forward", e))?;
        let prod = (&gate * &up)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger gate*up", e))?;
        self.down_proj
            .forward(&prod)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision merger down_proj forward", e))
    }
}

#[derive(Debug, Clone)]
pub struct GlmOcrVisionModel {
    cfg: GlmOcrVisionConfig,
    patch_embed: GlmOcrVisionPatchEmbed,
    rotary_pos_emb: GlmOcrVisionRotaryEmbedding,
    blocks: Vec<GlmOcrVisionBlock>,
    merger: GlmOcrVisionPatchMerger,
    downsample: Conv2d,
    post_layernorm: RmsNorm,
}

impl GlmOcrVisionModel {
    pub fn load(cfg: &GlmOcrVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let patch_embed = GlmOcrVisionPatchEmbed::load(cfg, vb.clone())?;

        let head_dim = cfg.hidden_size / cfg.num_heads;
        let rotary_pos_emb =
            GlmOcrVisionRotaryEmbedding::new(head_dim / 2, vb.device(), vb.dtype())?;

        let mut blocks = Vec::with_capacity(cfg.depth);
        let vb_blocks = vb.pp("blocks");
        for i in 0..cfg.depth {
            let block = GlmOcrVisionBlock::load_block(cfg, vb_blocks.pp(i))?;
            blocks.push(block);
        }

        let merger = GlmOcrVisionPatchMerger::load(cfg, vb.pp("merger"))?;

        let conv_cfg = Conv2dConfig {
            stride: cfg.spatial_merge_size,
            padding: 0,
            dilation: 1,
            groups: 1,
            cudnn_fwd_algo: None,
        };
        let downsample = candle_nn::conv2d(
            cfg.hidden_size,
            cfg.out_hidden_size,
            cfg.spatial_merge_size,
            conv_cfg,
            vb.pp("downsample"),
        )
        .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision downsample", e))?;

        let post_layernorm =
            rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("post_layernorm"))
                .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision post_layernorm", e))?;

        Ok(Self {
            cfg: cfg.clone(),
            patch_embed,
            rotary_pos_emb,
            blocks,
            merger,
            downsample,
            post_layernorm,
        })
    }

    fn rot_pos_emb(&self, grid_thw: (usize, usize, usize)) -> Result<Tensor, Error> {
        let (_t, h, w) = grid_thw;
        let merge = self.cfg.spatial_merge_size;
        let device = self.patch_embed.weight.device();

        // Use on_compute_device to handle Metal's lack of support for arange and broadcast_as
        let hpos = on_compute_device(device, |compute_device| {
            let hpos = Tensor::arange(0u32, h as u32, compute_device)?
                .reshape((h, 1))?
                .broadcast_as((h, w))?;
            hpos.reshape((h / merge, merge, w / merge, merge))?
                .permute((0, 2, 1, 3))?
                .flatten(0, 3)
        })
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision hpos computation failed",
                e,
            )
        })?;

        let wpos = on_compute_device(device, |compute_device| {
            let wpos = Tensor::arange(0u32, w as u32, compute_device)?
                .reshape((1, w))?
                .broadcast_as((h, w))?;
            wpos.reshape((h / merge, merge, w / merge, merge))?
                .permute((0, 2, 1, 3))?
                .flatten(0, 3)
        })
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision wpos computation failed",
                e,
            )
        })?;

        let pos_ids = Tensor::stack(&[&hpos, &wpos], D::Minus1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision pos_ids stack",
                e,
            )
        })?;

        let max_grid = h.max(w);
        let rotary_full = self.rotary_pos_emb.forward(max_grid)?;
        let pos_ids_flat = pos_ids.flatten(0, 1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision pos_ids flatten",
                e,
            )
        })?;
        let rotary = rotary_full
            .index_select(&pos_ids_flat, 0)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision rotary index_select",
                    e,
                )
            })?
            .reshape((
                pos_ids.dim(0).map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "GLM-OCR: vision pos_ids dim0",
                        e,
                    )
                })?,
                pos_ids.dim(1).map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "GLM-OCR: vision pos_ids dim1",
                        e,
                    )
                })?,
                rotary_full.dim(1).map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "GLM-OCR: vision rotary dim1",
                        e,
                    )
                })?,
            ))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision rotary reshape",
                    e,
                )
            })?
            .flatten(1, 2)
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision rotary flatten",
                    e,
                )
            })?;

        Ok(rotary)
    }

    pub fn forward(
        &self,
        pixel_values: &Tensor,
        grid_thw: (usize, usize, usize),
    ) -> Result<Tensor, Error> {
        let mut hidden_states = self.patch_embed.forward(pixel_values)?;

        let rotary_pos_emb = self.rot_pos_emb(grid_thw)?;
        let emb = Tensor::cat(&[&rotary_pos_emb, &rotary_pos_emb], D::Minus1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision rotary cat",
                e,
            )
        })?;
        let cos = emb.cos().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision cos",
                e,
            )
        })?;
        let sin = emb.sin().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision sin",
                e,
            )
        })?;

        for block in &self.blocks {
            hidden_states = block.forward(&hidden_states, &cos, &sin)?;
        }

        hidden_states = self
            .post_layernorm
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision post_layernorm", e))?;

        let merge = self.cfg.spatial_merge_size;
        let seq_len = hidden_states.dim(0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: vision seq_len",
                e,
            )
        })?;
        let hidden = hidden_states
            .reshape((
                seq_len / (merge * merge),
                merge,
                merge,
                self.cfg.hidden_size,
            ))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision merge reshape",
                    e,
                )
            })?
            .permute((0, 3, 1, 2))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision merge permute",
                    e,
                )
            })?;

        let hidden = self
            .downsample
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference("GLM-OCR", "vision downsample forward", e))?;
        let hidden = hidden
            .reshape((
                hidden.dim(0).map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "GLM-OCR: vision downsample dim0",
                        e,
                    )
                })?,
                self.cfg.out_hidden_size,
            ))
            .map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "GLM-OCR: vision downsample reshape",
                    e,
                )
            })?;

        self.merger.forward(&hidden)
    }
}
