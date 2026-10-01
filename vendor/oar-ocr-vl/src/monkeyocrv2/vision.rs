use super::config::MonkeyOcrV2VisionConfig;
use crate::attention::{
    VISION_CHUNKED_ATTN_CHUNK_SIZE, VISION_CHUNKED_ATTN_SEQ_THRESHOLD, chunked_vision_attention,
    flash_attention, scaled_dot_product_attention,
};
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{DType, IndexOp, Tensor};
use candle_nn::{
    LayerNorm, LayerNormConfig, Linear, Module, RmsNorm, VarBuilder, layer_norm, linear,
    linear_no_bias, rms_norm,
};

const MODEL_NAME: &str = "MonkeyOCRv2";

#[derive(Debug, Clone)]
struct PatchEmbed {
    weight: Tensor,
    bias: Tensor,
    norm: RmsNorm,
}

impl PatchEmbed {
    fn load(cfg: &MonkeyOcrV2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let patch_dim = cfg.num_channels * cfg.patch_size * cfg.patch_size;
        let vb = vb.pp("patch_embed").pp("patchifier");
        let weight = vb
            .get(
                (
                    cfg.embed_dim,
                    cfg.num_channels,
                    cfg.patch_size,
                    cfg.patch_size,
                ),
                "proj.weight",
            )
            .and_then(|weight| weight.reshape((cfg.embed_dim, patch_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load patch projection", e))?;
        let bias = vb
            .get(cfg.embed_dim, "proj.bias")
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load patch bias", e))?;
        let norm = rms_norm(cfg.embed_dim, cfg.rms_norm_eps, vb.pp("norm"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load patch RMSNorm", e))?;
        Ok(Self { weight, bias, norm })
    }

    fn forward(&self, patches: &Tensor) -> Result<Tensor, Error> {
        let patches = patches
            .to_dtype(self.weight.dtype())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "cast image patches", e))?;
        let hidden =
            patches
                .matmul(&self.weight.transpose(0, 1).map_err(|e| {
                    candle_to_ocr_inference(MODEL_NAME, "transpose patch weight", e)
                })?)
                .and_then(|hidden| hidden.broadcast_add(&self.bias))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "patch projection", e))?;
        self.norm
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "patch RMSNorm", e))
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
    fn load(cfg: &MonkeyOcrV2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let qkv = linear_no_bias(cfg.embed_dim, cfg.embed_dim * 3, vb.pp("attn").pp("qkv"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision QKV", e))?;
        let proj = linear_no_bias(cfg.embed_dim, cfg.embed_dim, vb.pp("attn").pp("proj"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision projection", e))?;
        let head_dim = cfg.head_dim()?;
        Ok(Self {
            qkv,
            proj,
            num_heads: cfg.num_attention_heads,
            head_dim,
            scale: 1.0 / (head_dim as f64).sqrt(),
        })
    }

    fn forward(&self, hidden: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let seq_len = hidden
            .dim(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read vision sequence length", e))?;
        let qkv = self
            .qkv
            .forward(hidden)
            .and_then(|qkv| qkv.reshape((seq_len, 3, self.num_heads, self.head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision QKV projection", e))?;
        let q = qkv
            .i((.., 0, .., ..))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "slice vision query", e))?;
        let k = qkv
            .i((.., 1, .., ..))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "slice vision key", e))?;
        let v = qkv
            .i((.., 2, .., ..))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "slice vision value", e))?;
        let (q, k) = apply_vision_rope(&q, &k, cos, sin)?;
        let q = q
            .transpose(0, 1)
            .and_then(|q| q.unsqueeze(0))
            .and_then(|q| q.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "layout vision query", e))?;
        let k = k
            .transpose(0, 1)
            .and_then(|k| k.unsqueeze(0))
            .and_then(|k| k.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "layout vision key", e))?;
        let v = v
            .transpose(0, 1)
            .and_then(|v| v.unsqueeze(0))
            .and_then(|v| v.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "layout vision value", e))?;

        let attention = match flash_attention(&q, &k, &v, self.scale, false)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision flash attention", e))?
        {
            Some(output) => output,
            None if seq_len > VISION_CHUNKED_ATTN_SEQ_THRESHOLD => {
                chunked_vision_attention(&q, &k, &v, self.scale, VISION_CHUNKED_ATTN_CHUNK_SIZE)
                    .map_err(|e| {
                        candle_to_ocr_inference(MODEL_NAME, "chunked vision attention", e)
                    })?
            }
            None => scaled_dot_product_attention(&q, &k, &v, None, self.scale, false)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision attention", e))?,
        };
        let attention = attention
            .transpose(1, 2)
            .and_then(|output| output.reshape((seq_len, self.num_heads * self.head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "reshape vision attention", e))?;
        self.proj
            .forward(&attention)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision output projection", e))
    }
}

fn apply_vision_rope(
    q: &Tensor,
    k: &Tensor,
    cos: &Tensor,
    sin: &Tensor,
) -> Result<(Tensor, Tensor), Error> {
    let q_dtype = q.dtype();
    let k_dtype = k.dtype();
    let q = q
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "cast vision query", e))?;
    let k = k
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "cast vision key", e))?;
    let cos = cos
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "cast vision cosine", e))?;
    let sin = sin
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "cast vision sine", e))?;
    let q_rotated = crate::utils::rotate_half(&q)?;
    let k_rotated = crate::utils::rotate_half(&k)?;
    let q = q
        .broadcast_mul(&cos)
        .and_then(|value| value.broadcast_add(&q_rotated.broadcast_mul(&sin)?))
        .and_then(|value| value.to_dtype(q_dtype))
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "apply query vision RoPE", e))?;
    let k = k
        .broadcast_mul(&cos)
        .and_then(|value| value.broadcast_add(&k_rotated.broadcast_mul(&sin)?))
        .and_then(|value| value.to_dtype(k_dtype))
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "apply key vision RoPE", e))?;
    Ok((q, k))
}

#[derive(Debug, Clone)]
struct VisionMlp {
    fc1: Linear,
    fc2: Linear,
    fc3: Linear,
}

impl VisionMlp {
    fn load(cfg: &MonkeyOcrV2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let vb = vb.pp("mlp");
        let fc1 = linear_no_bias(cfg.embed_dim, cfg.intermediate_size, vb.pp("fc1"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision MLP fc1", e))?;
        let fc2 = linear_no_bias(cfg.intermediate_size, cfg.embed_dim, vb.pp("fc2"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision MLP fc2", e))?;
        let fc3 = linear_no_bias(cfg.embed_dim, cfg.intermediate_size, vb.pp("fc3"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision MLP fc3", e))?;
        Ok(Self { fc1, fc2, fc3 })
    }

    fn forward(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let gate = self
            .fc1
            .forward(hidden)
            .and_then(|gate| candle_nn::ops::silu(&gate))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision MLP gate", e))?;
        let up = self
            .fc3
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision MLP up", e))?;
        let hidden =
            (gate * up).map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision MLP SwiGLU", e))?;
        self.fc2
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision MLP down", e))
    }
}

#[derive(Debug, Clone)]
struct VisionBlock {
    norm1: RmsNorm,
    norm2: RmsNorm,
    attention: VisionAttention,
    mlp: VisionMlp,
}

impl VisionBlock {
    fn load(cfg: &MonkeyOcrV2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let norm1 = rms_norm(cfg.embed_dim, cfg.rms_norm_eps, vb.pp("norm1"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision norm1", e))?;
        let norm2 = rms_norm(cfg.embed_dim, cfg.rms_norm_eps, vb.pp("norm2"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision norm2", e))?;
        let attention = VisionAttention::load(cfg, vb.clone())?;
        let mlp = VisionMlp::load(cfg, vb)?;
        Ok(Self {
            norm1,
            norm2,
            attention,
            mlp,
        })
    }

    fn forward(&self, hidden: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let normed = self
            .norm1
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision norm1", e))?;
        let attention = self.attention.forward(&normed, cos, sin)?;
        let hidden = (hidden + attention).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MonkeyOCRv2 vision attention residual",
                e,
            )
        })?;
        let normed = self
            .norm2
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision norm2", e))?;
        let mlp = self.mlp.forward(&normed)?;
        (hidden + mlp).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MonkeyOCRv2 vision MLP residual",
                e,
            )
        })
    }
}

#[derive(Debug, Clone)]
struct PatchMerger {
    norm: LayerNorm,
    mlp1: Linear,
    mlp2: Linear,
    merged_dim: usize,
    merge_group: usize,
}

impl PatchMerger {
    fn load(cfg: &MonkeyOcrV2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let vb = vb.pp("merger");
        let norm = layer_norm(
            cfg.embed_dim,
            LayerNormConfig {
                eps: 1e-6,
                ..Default::default()
            },
            vb.pp("ln_q"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load merger LayerNorm", e))?;
        let merge_group = cfg.spatial_merge_size * cfg.spatial_merge_size;
        let merged_dim = cfg.embed_dim * merge_group;
        let mlp1 = linear(merged_dim, merged_dim, vb.pp("mlp").pp("0"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load merger MLP 0", e))?;
        let mlp2 = linear(merged_dim, cfg.hidden_size, vb.pp("mlp").pp("2"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load merger MLP 2", e))?;
        Ok(Self {
            norm,
            mlp1,
            mlp2,
            merged_dim,
            merge_group,
        })
    }

    fn forward(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let num_patches = hidden
            .dim(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read merger patch count", e))?;
        if !num_patches.is_multiple_of(self.merge_group) {
            return Err(Error::InvalidInput {
                message: format!(
                    "MonkeyOCRv2 merger patch count {num_patches} is not divisible by {}",
                    self.merge_group
                ),
            });
        }
        let hidden = self
            .norm
            .forward(hidden)
            .and_then(|hidden| hidden.reshape((num_patches / self.merge_group, self.merged_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "merger norm/reshape", e))?;
        let hidden = self
            .mlp1
            .forward(&hidden)
            .and_then(|hidden| hidden.gelu_erf())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "merger MLP 0", e))?;
        self.mlp2
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "merger MLP 2", e))
    }
}

pub struct MonkeyOcrV2VisionModel {
    patch_embed: PatchEmbed,
    blocks: Vec<VisionBlock>,
    post_norm: Option<RmsNorm>,
    merger: PatchMerger,
    head_dim: usize,
    merge_size: usize,
}

impl MonkeyOcrV2VisionModel {
    pub fn load(cfg: &MonkeyOcrV2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        cfg.validate()?;
        let patch_embed = PatchEmbed::load(cfg, vb.clone())?;
        let mut blocks = Vec::with_capacity(cfg.num_hidden_layers);
        for index in 0..cfg.num_hidden_layers {
            blocks.push(VisionBlock::load(cfg, vb.pp("blocks").pp(index))?);
        }
        let post_norm = cfg
            .post_norm
            .then(|| rms_norm(cfg.embed_dim, cfg.rms_norm_eps, vb.pp("post_trunk_norm")))
            .transpose()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision post norm", e))?;
        let merger = PatchMerger::load(cfg, vb.clone())?;
        Ok(Self {
            patch_embed,
            blocks,
            post_norm,
            merger,
            head_dim: cfg.head_dim()?,
            merge_size: cfg.spatial_merge_size,
        })
    }

    pub fn forward(
        &self,
        pixel_values: &Tensor,
        grid_thw: (usize, usize, usize),
    ) -> Result<Tensor, Error> {
        let (grid_t, grid_h, grid_w) = grid_thw;
        if grid_t != 1
            || grid_h == 0
            || grid_w == 0
            || !grid_h.is_multiple_of(self.merge_size)
            || !grid_w.is_multiple_of(self.merge_size)
        {
            return Err(Error::InvalidInput {
                message: format!(
                    "MonkeyOCRv2 invalid image grid {grid_thw:?} for merge_size {}",
                    self.merge_size
                ),
            });
        }
        let expected = grid_t * grid_h * grid_w;
        let actual = pixel_values
            .dim(0)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read pixel patch count", e))?;
        if actual != expected {
            return Err(Error::InvalidInput {
                message: format!(
                    "MonkeyOCRv2 pixel patch count {actual} does not match grid {grid_thw:?} ({expected})"
                ),
            });
        }
        let (cos, sin) = build_vision_rope(grid_thw, self.merge_size, self.head_dim, pixel_values)?;
        let mut hidden = self.patch_embed.forward(pixel_values)?;
        for block in &self.blocks {
            hidden = block.forward(&hidden, &cos, &sin)?;
        }
        if let Some(norm) = &self.post_norm {
            hidden = norm
                .forward(&hidden)
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision post norm", e))?;
        }
        self.merger.forward(&hidden)
    }
}

fn build_vision_rope(
    grid_thw: (usize, usize, usize),
    merge_size: usize,
    head_dim: usize,
    like: &Tensor,
) -> Result<(Tensor, Tensor), Error> {
    let (grid_t, grid_h, grid_w) = grid_thw;
    let mut frequencies = Vec::with_capacity(grid_t * grid_h * grid_w * head_dim);
    for _ in 0..grid_t {
        for height_block in 0..(grid_h / merge_size) {
            for width_block in 0..(grid_w / merge_size) {
                for height_inner in 0..merge_size {
                    for width_inner in 0..merge_size {
                        let height = height_block * merge_size + height_inner;
                        let width = width_block * merge_size + width_inner;
                        let mut half = Vec::with_capacity(head_dim / 2);
                        for index in (0..head_dim / 2).step_by(2) {
                            let inv =
                                1.0_f32 / 10_000_f32.powf(index as f32 / (head_dim / 2) as f32);
                            half.push(height as f32 * inv);
                            half.push(width as f32 * inv);
                        }
                        frequencies.extend_from_slice(&half);
                        frequencies.extend_from_slice(&half);
                    }
                }
            }
        }
    }
    let num_patches = grid_t * grid_h * grid_w;
    let frequencies = Tensor::from_vec(frequencies, (num_patches, 1, head_dim), like.device())
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "build vision RoPE frequencies", e))?;
    let cos = frequencies
        .cos()
        .and_then(|value| value.to_dtype(like.dtype()))
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "build vision RoPE cosine", e))?;
    let sin = frequencies
        .sin()
        .and_then(|value| value.to_dtype(like.dtype()))
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "build vision RoPE sine", e))?;
    Ok((cos, sin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rope_follows_merge_grouped_patch_order() {
        let like = Tensor::zeros((24, 588), DType::F32, &candle_core::Device::Cpu).unwrap();
        let (cos, sin) = build_vision_rope((1, 4, 6), 2, 64, &like).unwrap();
        assert_eq!(cos.dims(), &[24, 1, 64]);
        assert_eq!(sin.dims(), &[24, 1, 64]);
        let first = cos.i((0, 0, ..)).unwrap().to_vec1::<f32>().unwrap();
        assert!(first.iter().all(|value| (*value - 1.0).abs() < 1e-6));
    }

    #[test]
    fn rope_interleaves_axes_before_rotate_half_duplication() {
        let like = Tensor::zeros((4, 588), DType::F32, &candle_core::Device::Cpu).unwrap();
        let (cos, _) = build_vision_rope((1, 2, 2), 1, 8, &like).unwrap();
        let bottom_right = cos.i((3, 0, ..)).unwrap().to_vec1::<f32>().unwrap();
        let expected = [
            1.0_f32.cos(),
            1.0_f32.cos(),
            0.01_f32.cos(),
            0.01_f32.cos(),
            1.0_f32.cos(),
            1.0_f32.cos(),
            0.01_f32.cos(),
            0.01_f32.cos(),
        ];
        assert!(
            bottom_right
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (*actual - expected).abs() < 1e-6)
        );
    }
}
