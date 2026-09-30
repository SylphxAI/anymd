use super::config::MinerUVisionConfig;
use crate::attention::{
    VISION_CHUNKED_ATTN_CHUNK_SIZE, VISION_CHUNKED_ATTN_SEQ_THRESHOLD, chunked_vision_attention,
    on_compute_device, scaled_dot_product_attention,
};
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{LayerNorm, LayerNormConfig, Linear, Module, VarBuilder, layer_norm, linear};

fn quick_gelu(xs: &Tensor) -> Result<Tensor, Error> {
    let scaled = (xs * 1.702).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: quick_gelu scale failed",
            e,
        )
    })?;
    let sig = candle_nn::ops::sigmoid(&scaled).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: quick_gelu sigmoid failed",
            e,
        )
    })?;
    (xs * sig).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: quick_gelu multiply failed",
            e,
        )
    })
}

#[derive(Debug, Clone, Copy)]
enum VisionAct {
    QuickGelu,
    Gelu,
    Silu,
}

impl VisionAct {
    fn from_str(name: &str) -> Result<Self, Error> {
        match name {
            "quick_gelu" => Ok(Self::QuickGelu),
            "gelu" | "gelu_new" | "gelu_pytorch_tanh" => Ok(Self::Gelu),
            "silu" => Ok(Self::Silu),
            _ => Err(Error::Config {
                message: format!("MinerU2.5: unsupported vision hidden_act '{name}'"),
            }),
        }
    }

    fn forward(self, xs: &Tensor) -> Result<Tensor, Error> {
        match self {
            Self::QuickGelu => quick_gelu(xs),
            Self::Gelu => xs.gelu_erf().map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "MinerU2.5: vision gelu failed",
                    e,
                )
            }),
            Self::Silu => candle_nn::ops::silu(xs).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "MinerU2.5: vision silu failed",
                    e,
                )
            }),
        }
    }
}

fn apply_rotary_pos_emb_vision(
    q: &Tensor,
    k: &Tensor,
    cos: &Tensor,
    sin: &Tensor,
) -> Result<(Tensor, Tensor), Error> {
    let orig_q_dtype = q.dtype();
    let orig_k_dtype = k.dtype();
    let q = q.to_dtype(DType::F32).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision q cast failed",
            e,
        )
    })?;
    let k = k.to_dtype(DType::F32).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision k cast failed",
            e,
        )
    })?;

    let cos = cos
        .unsqueeze(1)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision cos unsqueeze failed",
                e,
            )
        })?
        .to_dtype(DType::F32)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision cos cast failed",
                e,
            )
        })?;
    let sin = sin
        .unsqueeze(1)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision sin unsqueeze failed",
                e,
            )
        })?
        .to_dtype(DType::F32)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision sin cast failed",
                e,
            )
        })?;

    let q_rot = crate::utils::rotate_half(&q)?;
    let q_embed = q
        .broadcast_mul(&cos)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision q*cos failed",
                e,
            )
        })?
        .broadcast_add(&q_rot.broadcast_mul(&sin).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision rotate_half(q)*sin failed",
                e,
            )
        })?)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision q rope add failed",
                e,
            )
        })?;

    let k_rot = crate::utils::rotate_half(&k)?;
    let k_embed = k
        .broadcast_mul(&cos)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision k*cos failed",
                e,
            )
        })?
        .broadcast_add(&k_rot.broadcast_mul(&sin).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision rotate_half(k)*sin failed",
                e,
            )
        })?)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision k rope add failed",
                e,
            )
        })?;

    let q_embed = q_embed.to_dtype(orig_q_dtype).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision q_embed cast back failed",
            e,
        )
    })?;
    let k_embed = k_embed.to_dtype(orig_k_dtype).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision k_embed cast back failed",
            e,
        )
    })?;

    Ok((q_embed, k_embed))
}

#[derive(Debug, Clone)]
struct VisionRotaryEmbedding {
    inv_freq: Tensor,
    dim: usize,
}

impl VisionRotaryEmbedding {
    fn new(dim: usize, theta: f64, device: &Device) -> Result<Self, Error> {
        let inv_freq = crate::utils::vision_inv_freq(dim, theta, "MinerU2.5", device)?;
        Ok(Self { inv_freq, dim })
    }

    fn forward(&self, seqlen: usize, device: &Device) -> Result<Tensor, Error> {
        // Use on_compute_device to handle Metal's lack of support for arange
        on_compute_device(device, |compute_device| {
            let seq = Tensor::arange(0u32, seqlen as u32, compute_device)?.to_dtype(DType::F32)?;
            let inv = self
                .inv_freq
                .to_device(compute_device)?
                .to_dtype(DType::F32)?;
            seq.unsqueeze(1)?.matmul(&inv.unsqueeze(0)?)
        })
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision rope forward failed",
                e,
            )
        })
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

#[derive(Debug, Clone)]
struct PatchEmbed {
    weight: Tensor,
}

impl PatchEmbed {
    fn load(cfg: &MinerUVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let patch_dim =
            cfg.in_channels() * cfg.temporal_patch_size * cfg.patch_size * cfg.patch_size;
        let weight = match vb.get((cfg.embed_dim, patch_dim), "patch_embed.proj.weight") {
            Ok(weight) => weight,
            Err(_) => {
                let weight = vb
                    .get(
                        (
                            cfg.embed_dim,
                            cfg.in_channels(),
                            cfg.temporal_patch_size,
                            cfg.patch_size,
                            cfg.patch_size,
                        ),
                        "patch_embed.proj.weight",
                    )
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load patch_embed", e))?;
                weight
                    .reshape((cfg.embed_dim, patch_dim))
                    .map_err(|e| candle_to_ocr_inference("MinerU2.5", "reshape patch_embed", e))?
            }
        };
        Ok(Self { weight })
    }

    fn forward(&self, patches: &Tensor) -> Result<Tensor, Error> {
        let weight_t = self.weight.transpose(0, 1).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: patch_embed weight transpose failed",
                e,
            )
        })?;
        let patches = patches.to_dtype(self.weight.dtype()).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: patch_embed input cast failed",
                e,
            )
        })?;
        patches
            .matmul(&weight_t)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "patch_embed matmul", e))
    }
}

#[derive(Debug, Clone)]
struct VisionAttention {
    qkv: Linear,
    proj: Linear,
    num_heads: usize,
    head_dim: usize,
    scale: f64,
}

impl VisionAttention {
    fn load(cfg: &MinerUVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let qkv = linear(cfg.embed_dim, cfg.embed_dim * 3, vb.pp("attn.qkv"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load vision qkv", e))?;
        let proj = linear(cfg.embed_dim, cfg.embed_dim, vb.pp("attn.proj"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load vision proj", e))?;

        if !cfg.embed_dim.is_multiple_of(cfg.num_heads) {
            return Err(Error::Config {
                message: format!(
                    "MinerU2.5: vision embed_dim {} not divisible by num_heads {}",
                    cfg.embed_dim, cfg.num_heads
                ),
            });
        }

        let head_dim = cfg.embed_dim / cfg.num_heads;
        Ok(Self {
            qkv,
            proj,
            num_heads: cfg.num_heads,
            head_dim,
            scale: 1.0 / (head_dim as f64).sqrt(),
        })
    }

    fn forward(&self, hidden_states: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let seq_len = hidden_states
            .dim(0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision hidden_states dim", e))?;
        let qkv = self
            .qkv
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision qkv", e))?;
        let qkv = qkv
            .reshape((seq_len, 3, self.num_heads, self.head_dim))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision qkv reshape", e))?;
        let q = qkv
            .i((.., 0, .., ..))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision q slice", e))?;
        let k = qkv
            .i((.., 1, .., ..))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision k slice", e))?;
        let v = qkv
            .i((.., 2, .., ..))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision v slice", e))?;

        let (q, k) = apply_rotary_pos_emb_vision(&q, &k, cos, sin)?;

        let q = q
            .transpose(0, 1)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision q transpose", e))?
            .unsqueeze(0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision q unsqueeze", e))?
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision q contiguous", e))?;
        let k = k
            .transpose(0, 1)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision k transpose", e))?
            .unsqueeze(0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision k unsqueeze", e))?
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision k contiguous", e))?;
        let v = v
            .transpose(0, 1)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision v transpose", e))?
            .unsqueeze(0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision v unsqueeze", e))?
            .contiguous()
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision v contiguous", e))?;

        let seq_len = q
            .dim(2)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision q dim", e))?;
        let attn = if seq_len > VISION_CHUNKED_ATTN_SEQ_THRESHOLD {
            chunked_vision_attention(&q, &k, &v, self.scale, VISION_CHUNKED_ATTN_CHUNK_SIZE)
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision attention", e))?
        } else {
            scaled_dot_product_attention(&q, &k, &v, None, self.scale, false)
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision attention", e))?
        };
        let attn = attn
            .transpose(1, 2)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision attn transpose", e))?
            .reshape((seq_len, self.num_heads * self.head_dim))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision attn reshape", e))?;
        self.proj
            .forward(&attn)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision proj", e))
    }
}

#[derive(Debug, Clone)]
struct VisionMlp {
    fc1: Linear,
    fc2: Linear,
    act: VisionAct,
}

impl VisionMlp {
    fn load(cfg: &MinerUVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let hidden_dim = cfg.mlp_hidden_dim();
        let fc1 = linear(cfg.embed_dim, hidden_dim, vb.pp("mlp.fc1"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load vision fc1", e))?;
        let fc2 = linear(hidden_dim, cfg.embed_dim, vb.pp("mlp.fc2"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load vision fc2", e))?;
        let act = VisionAct::from_str(cfg.hidden_act.as_str())?;
        Ok(Self { fc1, fc2, act })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor, Error> {
        let x = self
            .fc1
            .forward(xs)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision fc1", e))?;
        let x = self.act.forward(&x)?;
        self.fc2
            .forward(&x)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision fc2", e))
    }
}

#[derive(Debug, Clone)]
struct VisionBlock {
    norm1: LayerNorm,
    norm2: LayerNorm,
    attn: VisionAttention,
    mlp: VisionMlp,
}

impl VisionBlock {
    fn load(cfg: &MinerUVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let norm_cfg = LayerNormConfig {
            eps: 1e-6,
            ..Default::default()
        };
        let norm1 = layer_norm(cfg.embed_dim, norm_cfg, vb.pp("norm1"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load vision norm1", e))?;
        let norm2 = layer_norm(cfg.embed_dim, norm_cfg, vb.pp("norm2"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load vision norm2", e))?;
        let attn = VisionAttention::load(cfg, vb.clone())?;
        let mlp = VisionMlp::load(cfg, vb)?;
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
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision norm1 forward", e))?;
        let attn_out = self.attn.forward(&normed, cos, sin)?;
        let hidden_states = (hidden_states + attn_out).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision attn residual failed",
                e,
            )
        })?;

        let normed = self
            .norm2
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision norm2 forward", e))?;
        let mlp_out = self.mlp.forward(&normed)?;
        (hidden_states + mlp_out).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: vision mlp residual failed",
                e,
            )
        })
    }
}

#[derive(Debug, Clone)]
struct PatchMerger {
    ln_q: LayerNorm,
    mlp1: Linear,
    mlp2: Linear,
    merge_size: usize,
    hidden_size: usize,
}

impl PatchMerger {
    fn load(cfg: &MinerUVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let norm_cfg = LayerNormConfig {
            eps: 1e-6,
            ..Default::default()
        };
        let ln_q = layer_norm(cfg.embed_dim, norm_cfg, vb.pp("merger.ln_q"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load merger ln_q", e))?;
        let hidden_size = cfg.embed_dim * cfg.spatial_merge_size * cfg.spatial_merge_size;
        let mlp1 = linear(hidden_size, hidden_size, vb.pp("merger.mlp.0"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load merger mlp1", e))?;
        let mlp2 = linear(hidden_size, cfg.hidden_size, vb.pp("merger.mlp.2"))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "load merger mlp2", e))?;
        Ok(Self {
            ln_q,
            mlp1,
            mlp2,
            merge_size: cfg.spatial_merge_size,
            hidden_size,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let num_patches = x
            .dim(0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "merger dim", e))?;
        let group = self.merge_size * self.merge_size;
        if num_patches % group != 0 {
            return Err(Error::InvalidInput {
                message: format!(
                    "MinerU2.5: merger expects num_patches divisible by {}, got {}",
                    group, num_patches
                ),
            });
        }
        let x = self
            .ln_q
            .forward(x)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "merger ln_q", e))?;
        let x = x
            .reshape((num_patches / group, self.hidden_size))
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "merger reshape", e))?;
        let x = self
            .mlp1
            .forward(&x)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "merger mlp1", e))?;
        let x = x.gelu_erf().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU2.5: merger gelu failed",
                e,
            )
        })?;
        self.mlp2
            .forward(&x)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "merger mlp2", e))
    }
}

pub struct MinerUVisionModel {
    patch_embed: PatchEmbed,
    blocks: Vec<VisionBlock>,
    merger: Option<PatchMerger>,
    rotary_pos_emb: VisionRotaryEmbedding,
    spatial_merge_size: usize,
}

impl MinerUVisionModel {
    pub fn load(cfg: &MinerUVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        Self::load_inner(cfg, vb, true)
    }

    /// Load only the patch-embedding + transformer backbone, skipping the
    /// `merger` head. Used by checkpoints (e.g. MinerU-Diffusion) that set the
    /// vision merger to identity and project the raw per-patch hidden states
    /// with an external abstractor instead. Pair with [`forward_tokens`].
    ///
    /// [`forward_tokens`]: MinerUVisionModel::forward_tokens
    pub(crate) fn load_backbone(cfg: &MinerUVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        Self::load_inner(cfg, vb, false)
    }

    fn load_inner(
        cfg: &MinerUVisionConfig,
        vb: VarBuilder,
        with_merger: bool,
    ) -> Result<Self, Error> {
        let patch_embed = PatchEmbed::load(cfg, vb.clone())?;
        let mut blocks = Vec::with_capacity(cfg.depth);
        for i in 0..cfg.depth {
            let block_vb = vb.pp(format!("blocks.{i}"));
            blocks.push(VisionBlock::load(cfg, block_vb)?);
        }
        let merger = if with_merger {
            Some(PatchMerger::load(cfg, vb.clone())?)
        } else {
            None
        };
        let head_dim = cfg.embed_dim / cfg.num_heads;
        if !head_dim.is_multiple_of(2) {
            return Err(Error::Config {
                message: format!(
                    "MinerU2.5: head_dim {} must be even for rotary embeddings",
                    head_dim
                ),
            });
        }
        let rotary_pos_emb = VisionRotaryEmbedding::new(head_dim / 2, 10000.0, vb.device())?;
        Ok(Self {
            patch_embed,
            blocks,
            merger,
            rotary_pos_emb,
            spatial_merge_size: cfg.spatial_merge_size,
        })
    }

    /// Run the patch embedding + transformer blocks per image and return the
    /// per-patch hidden states (one row per patch, dim `embed_dim`), without
    /// applying the spatial merger. Attention is isolated per image.
    pub(crate) fn forward_tokens(
        &self,
        pixel_values: &Tensor,
        grid_thw: &[(usize, usize, usize)],
    ) -> Result<Tensor, Error> {
        let max_grid = grid_thw
            .iter()
            .map(|(_, h, w)| (*h).max(*w))
            .max()
            .unwrap_or(0);
        let rotary_full = self
            .rotary_pos_emb
            .forward(max_grid, pixel_values.device())?;
        let freq_dim = self.rotary_pos_emb.dim() / 2;

        let mut outputs: Vec<Tensor> = Vec::with_capacity(grid_thw.len());
        let mut offset = 0usize;
        for &(t, h, w) in grid_thw {
            let num_patches = t * h * w;
            let patches = pixel_values
                .narrow(0, offset, num_patches)
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision narrow patches", e))?;
            offset += num_patches;

            let mut hidden = self.patch_embed.forward(&patches)?;
            let (cos, sin) = build_vision_pos_emb(
                &rotary_full,
                freq_dim,
                t,
                h,
                w,
                self.spatial_merge_size,
                pixel_values.device(),
            )?;
            for block in &self.blocks {
                hidden = block.forward(&hidden, &cos, &sin)?;
            }
            outputs.push(hidden);
        }

        let refs: Vec<&Tensor> = outputs.iter().collect();
        Tensor::cat(&refs, 0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision tokens cat", e))
    }

    pub fn forward(
        &self,
        pixel_values: &Tensor,
        grid_thw: &[(usize, usize, usize)],
    ) -> Result<Tensor, Error> {
        let merger = self.merger.as_ref().ok_or_else(|| Error::Config {
            message: "MinerU2.5: vision merger not loaded (backbone-only model)".to_string(),
        })?;
        let mut outputs: Vec<Tensor> = Vec::with_capacity(grid_thw.len());

        let max_grid = grid_thw
            .iter()
            .map(|(_, h, w)| (*h).max(*w))
            .max()
            .unwrap_or(0);
        let rotary_full = self
            .rotary_pos_emb
            .forward(max_grid, pixel_values.device())?;
        let freq_dim = self.rotary_pos_emb.dim() / 2;

        let mut offset = 0usize;
        for &(t, h, w) in grid_thw {
            let num_patches = t * h * w;
            let patches = pixel_values
                .narrow(0, offset, num_patches)
                .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision narrow patches", e))?;
            offset += num_patches;

            let mut hidden = self.patch_embed.forward(&patches)?;

            let (cos, sin) = build_vision_pos_emb(
                &rotary_full,
                freq_dim,
                t,
                h,
                w,
                self.spatial_merge_size,
                pixel_values.device(),
            )?;

            for block in &self.blocks {
                hidden = block.forward(&hidden, &cos, &sin)?;
            }

            let merged = merger.forward(&hidden)?;
            outputs.push(merged);
        }

        let refs: Vec<&Tensor> = outputs.iter().collect();
        Tensor::cat(&refs, 0)
            .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision outputs cat", e))
    }
}

fn build_vision_pos_emb(
    rotary_full: &Tensor,
    freq_dim: usize,
    t: usize,
    h: usize,
    w: usize,
    merge_size: usize,
    device: &Device,
) -> Result<(Tensor, Tensor), Error> {
    let mut hpos: Vec<i64> = Vec::with_capacity(t * h * w);
    let mut wpos: Vec<i64> = Vec::with_capacity(t * h * w);
    for _ in 0..t {
        for hb in 0..(h / merge_size) {
            for wb in 0..(w / merge_size) {
                for h_inner in 0..merge_size {
                    for w_inner in 0..merge_size {
                        hpos.push((hb * merge_size + h_inner) as i64);
                        wpos.push((wb * merge_size + w_inner) as i64);
                    }
                }
            }
        }
    }

    let num_patches = hpos.len();
    let hpos = Tensor::from_vec(hpos, (num_patches, 1usize), device).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision hpos tensor failed",
            e,
        )
    })?;
    let wpos = Tensor::from_vec(wpos, (num_patches, 1usize), device).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision wpos tensor failed",
            e,
        )
    })?;
    let hpos = hpos.broadcast_as((num_patches, freq_dim)).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision hpos broadcast failed",
            e,
        )
    })?;
    let hpos = hpos.contiguous().map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision hpos contiguous failed",
            e,
        )
    })?;
    let wpos = wpos.broadcast_as((num_patches, freq_dim)).map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision wpos broadcast failed",
            e,
        )
    })?;
    let wpos = wpos.contiguous().map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision wpos contiguous failed",
            e,
        )
    })?;

    let freqs_h = rotary_full
        .gather(&hpos, 0)
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision gather h", e))?;
    let freqs_w = rotary_full
        .gather(&wpos, 0)
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision gather w", e))?;
    let rotary = Tensor::cat(&[&freqs_h, &freqs_w], candle_core::D::Minus1)
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision pos cat", e))?;
    let emb = Tensor::cat(&[&rotary, &rotary], candle_core::D::Minus1)
        .map_err(|e| candle_to_ocr_inference("MinerU2.5", "vision emb cat", e))?;
    let cos = emb.cos().map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision cos failed",
            e,
        )
    })?;
    let sin = emb.sin().map_err(|e| {
        candle_to_ocr_processing(
            crate::error::ProcessingStage::TensorOperation,
            "MinerU2.5: vision sin failed",
            e,
        )
    })?;
    Ok((cos, sin))
}
