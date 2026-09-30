use super::config::OvisOcr2VisionConfig;
use crate::attention::{
    VISION_CHUNKED_ATTN_CHUNK_SIZE, VISION_CHUNKED_ATTN_SEQ_THRESHOLD, chunked_vision_attention,
    flash_attention, on_compute_device, scaled_dot_product_attention,
};
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{
    Activation, LayerNorm, LayerNormConfig, Linear, Module, VarBuilder, layer_norm, linear,
};

#[derive(Debug, Clone)]
struct VisionPatchEmbed {
    weight: Tensor,
    bias: Tensor,
}

impl VisionPatchEmbed {
    fn load(cfg: &OvisOcr2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let vb = vb.pp("patch_embed").pp("proj");
        let patch_dim = cfg.in_channels * cfg.temporal_patch_size * cfg.patch_size * cfg.patch_size;
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
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision patch weight", e))?
            .reshape((cfg.hidden_size, patch_dim))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "reshape vision patch weight", e))?;
        let bias = vb
            .get(cfg.hidden_size, "bias")
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision patch bias", e))?;
        Ok(Self { weight, bias })
    }

    fn forward(&self, patches: &Tensor) -> Result<Tensor, Error> {
        let patches = patches
            .to_dtype(self.weight.dtype())
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "cast vision patches", e))?;
        let weight_t = self
            .weight
            .transpose(0, 1)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "transpose patch weight", e))?;
        patches
            .matmul(&weight_t)
            .and_then(|output| output.broadcast_add(&self.bias))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision patch embedding", e))
    }
}

#[derive(Debug, Clone)]
struct VisionRotaryEmbedding {
    inv_freq: Tensor,
}

impl VisionRotaryEmbedding {
    fn new(dim: usize, device: &Device) -> Result<Self, Error> {
        let inv_freq = crate::utils::vision_inv_freq(dim, 10_000.0, "OvisOCR2", device)?;
        Ok(Self { inv_freq })
    }

    fn forward(&self, sequence_length: usize) -> Result<Tensor, Error> {
        let device = self.inv_freq.device();
        on_compute_device(device, |compute_device| {
            let positions = Tensor::arange(0u32, sequence_length as u32, compute_device)?
                .to_dtype(DType::F32)?;
            let inv_freq = self
                .inv_freq
                .to_device(compute_device)?
                .to_dtype(DType::F32)?;
            positions.unsqueeze(1)?.matmul(&inv_freq.unsqueeze(0)?)
        })
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "OvisOCR2: build vision rotary frequency table",
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
    let q_dtype = q.dtype();
    let k_dtype = k.dtype();
    let q = q
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "cast vision query", e))?;
    let k = k
        .to_dtype(DType::F32)
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "cast vision key", e))?;
    let cos = cos
        .unsqueeze(1)
        .and_then(|value| value.to_dtype(DType::F32))
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "prepare vision rotary cos", e))?;
    let sin = sin
        .unsqueeze(1)
        .and_then(|value| value.to_dtype(DType::F32))
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "prepare vision rotary sin", e))?;

    let rotate_q = crate::utils::rotate_half(&q)?;
    let rotate_k = crate::utils::rotate_half(&k)?;
    let q = q
        .broadcast_mul(&cos)
        .and_then(|value| value.broadcast_add(&rotate_q.broadcast_mul(&sin)?))
        .and_then(|value| value.to_dtype(q_dtype))
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "apply query vision rotary", e))?;
    let k = k
        .broadcast_mul(&cos)
        .and_then(|value| value.broadcast_add(&rotate_k.broadcast_mul(&sin)?))
        .and_then(|value| value.to_dtype(k_dtype))
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "apply key vision rotary", e))?;
    Ok((q, k))
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
    fn load(cfg: &OvisOcr2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let head_dim = cfg.head_dim()?;
        let qkv = linear(
            cfg.hidden_size,
            cfg.hidden_size * 3,
            vb.pp("attn").pp("qkv"),
        )
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision qkv", e))?;
        let proj = linear(cfg.hidden_size, cfg.hidden_size, vb.pp("attn").pp("proj"))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision projection", e))?;
        Ok(Self {
            qkv,
            proj,
            num_heads: cfg.num_heads,
            head_dim,
            scale: 1.0 / (head_dim as f64).sqrt(),
        })
    }

    fn forward(&self, hidden_states: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let sequence_length = hidden_states
            .dim(0)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision sequence length", e))?;
        let qkv = self
            .qkv
            .forward(hidden_states)
            .and_then(|qkv| qkv.reshape((sequence_length, 3, self.num_heads, self.head_dim)))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision qkv projection", e))?;
        let q = qkv
            .i((.., 0, .., ..))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "slice vision query", e))?;
        let k = qkv
            .i((.., 1, .., ..))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "slice vision key", e))?;
        let v = qkv
            .i((.., 2, .., ..))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "slice vision value", e))?;
        let (q, k) = apply_rotary_pos_emb_vision(&q, &k, cos, sin)?;

        let q = q
            .transpose(0, 1)
            .and_then(|value| value.unsqueeze(0))
            .and_then(|value| value.contiguous())
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "layout vision query", e))?;
        let k = k
            .transpose(0, 1)
            .and_then(|value| value.unsqueeze(0))
            .and_then(|value| value.contiguous())
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "layout vision key", e))?;
        let v = v
            .transpose(0, 1)
            .and_then(|value| value.unsqueeze(0))
            .and_then(|value| value.contiguous())
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "layout vision value", e))?;

        let attention = match flash_attention(&q, &k, &v, self.scale, false)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision flash attention", e))?
        {
            Some(output) => output,
            None if sequence_length > VISION_CHUNKED_ATTN_SEQ_THRESHOLD => {
                chunked_vision_attention(&q, &k, &v, self.scale, VISION_CHUNKED_ATTN_CHUNK_SIZE)
                    .map_err(|e| {
                        candle_to_ocr_inference("OvisOCR2", "chunked vision attention", e)
                    })?
            }
            None => scaled_dot_product_attention(&q, &k, &v, None, self.scale, false)
                .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision attention", e))?,
        };
        let attention = attention
            .transpose(1, 2)
            .and_then(|value| value.reshape((sequence_length, self.num_heads * self.head_dim)))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "reshape vision attention", e))?;
        self.proj
            .forward(&attention)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision output projection", e))
    }
}

#[derive(Debug, Clone)]
struct VisionMlp {
    linear_fc1: Linear,
    linear_fc2: Linear,
    activation: Activation,
}

impl VisionMlp {
    fn load(cfg: &OvisOcr2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let linear_fc1 = linear(
            cfg.hidden_size,
            cfg.intermediate_size,
            vb.pp("mlp").pp("linear_fc1"),
        )
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision MLP fc1", e))?;
        let linear_fc2 = linear(
            cfg.intermediate_size,
            cfg.hidden_size,
            vb.pp("mlp").pp("linear_fc2"),
        )
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision MLP fc2", e))?;
        Ok(Self {
            linear_fc1,
            linear_fc2,
            activation: cfg.hidden_act,
        })
    }

    fn forward(&self, hidden_states: &Tensor) -> Result<Tensor, Error> {
        let hidden_states = self
            .linear_fc1
            .forward(hidden_states)
            .and_then(|value| self.activation.forward(&value))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision MLP fc1", e))?;
        self.linear_fc2
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision MLP fc2", e))
    }
}

#[derive(Debug, Clone)]
struct VisionBlock {
    norm1: LayerNorm,
    norm2: LayerNorm,
    attention: VisionAttention,
    mlp: VisionMlp,
}

impl VisionBlock {
    fn load(cfg: &OvisOcr2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let norm_cfg = LayerNormConfig {
            eps: 1e-6,
            ..Default::default()
        };
        let norm1 = layer_norm(cfg.hidden_size, norm_cfg, vb.pp("norm1"))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision norm1", e))?;
        let norm2 = layer_norm(cfg.hidden_size, norm_cfg, vb.pp("norm2"))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision norm2", e))?;
        let attention = VisionAttention::load(cfg, vb.clone())?;
        let mlp = VisionMlp::load(cfg, vb)?;
        Ok(Self {
            norm1,
            norm2,
            attention,
            mlp,
        })
    }

    fn forward(&self, hidden_states: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor, Error> {
        let normed = self
            .norm1
            .forward(hidden_states)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision norm1", e))?;
        let attention = self.attention.forward(&normed, cos, sin)?;
        let hidden_states = (hidden_states + attention).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "OvisOCR2: vision attention residual",
                e,
            )
        })?;
        let normed = self
            .norm2
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision norm2", e))?;
        let mlp = self.mlp.forward(&normed)?;
        (hidden_states + mlp).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "OvisOCR2: vision MLP residual",
                e,
            )
        })
    }
}

#[derive(Debug, Clone)]
struct VisionPatchMerger {
    norm: LayerNorm,
    linear_fc1: Linear,
    linear_fc2: Linear,
    merged_hidden_size: usize,
    merge_group: usize,
}

impl VisionPatchMerger {
    fn load(cfg: &OvisOcr2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let vb = vb.pp("merger");
        let norm_cfg = LayerNormConfig {
            eps: 1e-6,
            ..Default::default()
        };
        let norm = layer_norm(cfg.hidden_size, norm_cfg, vb.pp("norm"))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision merger norm", e))?;
        let merge_group = cfg.spatial_merge_size * cfg.spatial_merge_size;
        let merged_hidden_size = cfg.hidden_size * merge_group;
        let linear_fc1 = linear(merged_hidden_size, merged_hidden_size, vb.pp("linear_fc1"))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision merger fc1", e))?;
        let linear_fc2 = linear(merged_hidden_size, cfg.out_hidden_size, vb.pp("linear_fc2"))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "load vision merger fc2", e))?;
        Ok(Self {
            norm,
            linear_fc1,
            linear_fc2,
            merged_hidden_size,
            merge_group,
        })
    }

    fn forward(&self, hidden_states: &Tensor) -> Result<Tensor, Error> {
        let num_patches = hidden_states
            .dim(0)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision merger patch count", e))?;
        if !num_patches.is_multiple_of(self.merge_group) {
            return Err(Error::InvalidInput {
                message: format!(
                    "OvisOCR2 vision merger expected patch count divisible by {}, got {num_patches}",
                    self.merge_group
                ),
            });
        }
        let hidden_states = self
            .norm
            .forward(hidden_states)
            .and_then(|value| {
                value.reshape((num_patches / self.merge_group, self.merged_hidden_size))
            })
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision merger norm/reshape", e))?;
        let hidden_states = self
            .linear_fc1
            .forward(&hidden_states)
            .and_then(|value| value.gelu_erf())
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision merger fc1", e))?;
        self.linear_fc2
            .forward(&hidden_states)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision merger fc2", e))
    }
}

pub struct OvisOcr2VisionModel {
    patch_embed: VisionPatchEmbed,
    position_embedding: Tensor,
    position_grid_size: usize,
    blocks: Vec<VisionBlock>,
    merger: VisionPatchMerger,
    rotary_embedding: VisionRotaryEmbedding,
    spatial_merge_size: usize,
}

impl OvisOcr2VisionModel {
    pub fn load(cfg: &OvisOcr2VisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        cfg.validate()?;
        let patch_embed = VisionPatchEmbed::load(cfg, vb.clone())?;
        let position_embedding = vb
            .get(
                (cfg.num_position_embeddings, cfg.hidden_size),
                "pos_embed.weight",
            )
            .map_err(|e| {
                candle_to_ocr_inference("OvisOCR2", "load vision position embedding", e)
            })?;
        let position_grid_size = cfg.position_grid_size()?;
        let mut blocks = Vec::with_capacity(cfg.depth);
        for index in 0..cfg.depth {
            blocks.push(VisionBlock::load(cfg, vb.pp("blocks").pp(index))?);
        }
        let merger = VisionPatchMerger::load(cfg, vb.clone())?;
        let rotary_embedding = VisionRotaryEmbedding::new(cfg.head_dim()? / 2, vb.device())?;
        Ok(Self {
            patch_embed,
            position_embedding,
            position_grid_size,
            blocks,
            merger,
            rotary_embedding,
            spatial_merge_size: cfg.spatial_merge_size,
        })
    }

    pub fn forward(
        &self,
        pixel_values: &Tensor,
        grid_thw: (usize, usize, usize),
    ) -> Result<Tensor, Error> {
        let (grid_t, grid_h, grid_w) = grid_thw;
        if grid_t == 0 || grid_h == 0 || grid_w == 0 {
            return Err(Error::InvalidInput {
                message: format!("OvisOCR2 vision grid must be non-zero, got {grid_thw:?}"),
            });
        }
        if !grid_h.is_multiple_of(self.spatial_merge_size)
            || !grid_w.is_multiple_of(self.spatial_merge_size)
        {
            return Err(Error::InvalidInput {
                message: format!(
                    "OvisOCR2 vision grid {grid_h}x{grid_w} must be divisible by merge_size {}",
                    self.spatial_merge_size
                ),
            });
        }
        let num_patches = grid_t
            .checked_mul(grid_h)
            .and_then(|value| value.checked_mul(grid_w))
            .ok_or_else(|| Error::InvalidInput {
                message: "OvisOCR2 vision patch count overflow".to_string(),
            })?;
        if pixel_values.dim(0).map_err(|e| {
            candle_to_ocr_inference("OvisOCR2", "read vision pixel_values length", e)
        })? != num_patches
        {
            return Err(Error::InvalidInput {
                message: format!(
                    "OvisOCR2 pixel_values patch count ({}) does not match grid {grid_thw:?} ({num_patches})",
                    pixel_values.dims().first().copied().unwrap_or(0)
                ),
            });
        }

        let mut hidden_states = self.patch_embed.forward(pixel_values)?;
        let position_embedding = interpolate_position_embedding(
            &self.position_embedding,
            self.position_grid_size,
            grid_thw,
            self.spatial_merge_size,
        )?;
        hidden_states = hidden_states
            .broadcast_add(&position_embedding)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "add vision position embedding", e))?;

        let (cos, sin) = build_vision_rotary_embeddings(
            &self.rotary_embedding,
            grid_thw,
            self.spatial_merge_size,
            pixel_values.device(),
        )?;
        for block in &self.blocks {
            hidden_states = block.forward(&hidden_states, &cos, &sin)?;
        }
        self.merger.forward(&hidden_states)
    }
}

fn merge_grouped_spatial_coordinates(
    grid_thw: (usize, usize, usize),
    merge_size: usize,
) -> Result<Vec<(usize, usize)>, Error> {
    let (grid_t, grid_h, grid_w) = grid_thw;
    if grid_t == 0
        || grid_h == 0
        || grid_w == 0
        || merge_size == 0
        || !grid_h.is_multiple_of(merge_size)
        || !grid_w.is_multiple_of(merge_size)
    {
        return Err(Error::InvalidInput {
            message: format!(
                "OvisOCR2 invalid merge-grouped grid: grid={grid_thw:?}, merge={merge_size}"
            ),
        });
    }
    let num_patches = grid_t
        .checked_mul(grid_h)
        .and_then(|value| value.checked_mul(grid_w))
        .ok_or_else(|| Error::InvalidInput {
            message: "OvisOCR2 merge-grouped patch count overflow".to_string(),
        })?;
    let mut coordinates = Vec::with_capacity(num_patches);
    for _ in 0..grid_t {
        for height_block in 0..(grid_h / merge_size) {
            for width_block in 0..(grid_w / merge_size) {
                for height_inner in 0..merge_size {
                    for width_inner in 0..merge_size {
                        coordinates.push((
                            height_block * merge_size + height_inner,
                            width_block * merge_size + width_inner,
                        ));
                    }
                }
            }
        }
    }
    Ok(coordinates)
}

fn interpolate_position_embedding(
    position_embedding: &Tensor,
    base_grid_size: usize,
    grid_thw: (usize, usize, usize),
    merge_size: usize,
) -> Result<Tensor, Error> {
    let (_, grid_h, grid_w) = grid_thw;
    if base_grid_size == 0 {
        return Err(Error::InvalidInput {
            message: format!(
                "OvisOCR2 invalid learned-position grid: base={base_grid_size}, grid={grid_thw:?}, merge={merge_size}"
            ),
        });
    }
    let (num_positions, hidden_size) = position_embedding
        .dims2()
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "position embedding shape", e))?;
    if num_positions != base_grid_size * base_grid_size {
        return Err(Error::Config {
            message: format!(
                "OvisOCR2 position embedding rows ({num_positions}) do not match base grid {base_grid_size}x{base_grid_size}"
            ),
        });
    }

    let coordinates = merge_grouped_spatial_coordinates(grid_thw, merge_size)?;
    let num_patches = coordinates.len();
    let mut index00 = Vec::with_capacity(num_patches);
    let mut index01 = Vec::with_capacity(num_patches);
    let mut index10 = Vec::with_capacity(num_patches);
    let mut index11 = Vec::with_capacity(num_patches);
    let mut weight00 = Vec::with_capacity(num_patches);
    let mut weight01 = Vec::with_capacity(num_patches);
    let mut weight10 = Vec::with_capacity(num_patches);
    let mut weight11 = Vec::with_capacity(num_patches);

    for (height, width) in coordinates {
        let source_h = if grid_h == 1 {
            0.0
        } else {
            height as f32 * (base_grid_size - 1) as f32 / (grid_h - 1) as f32
        };
        let source_w = if grid_w == 1 {
            0.0
        } else {
            width as f32 * (base_grid_size - 1) as f32 / (grid_w - 1) as f32
        };
        let h0 = source_h.floor() as usize;
        let w0 = source_w.floor() as usize;
        let h1 = (h0 + 1).min(base_grid_size - 1);
        let w1 = (w0 + 1).min(base_grid_size - 1);
        let dh = source_h - h0 as f32;
        let dw = source_w - w0 as f32;

        index00.push((h0 * base_grid_size + w0) as u32);
        index01.push((h0 * base_grid_size + w1) as u32);
        index10.push((h1 * base_grid_size + w0) as u32);
        index11.push((h1 * base_grid_size + w1) as u32);
        weight00.push((1.0 - dh) * (1.0 - dw));
        weight01.push((1.0 - dh) * dw);
        weight10.push(dh * (1.0 - dw));
        weight11.push(dh * dw);
    }

    let weighted = |indices: Vec<u32>, weights: Vec<f32>| -> Result<Tensor, Error> {
        let indices =
            Tensor::from_vec(indices, num_patches, position_embedding.device()).map_err(|e| {
                candle_to_ocr_inference("OvisOCR2", "position interpolation indices", e)
            })?;
        let weights = Tensor::from_vec(weights, (num_patches, 1), position_embedding.device())
            .and_then(|weights| weights.to_dtype(position_embedding.dtype()))
            .and_then(|weights| weights.broadcast_as((num_patches, hidden_size)))
            .map_err(|e| {
                candle_to_ocr_inference("OvisOCR2", "position interpolation weights", e)
            })?;
        position_embedding
            .index_select(&indices, 0)
            .and_then(|selected| selected.broadcast_mul(&weights))
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "interpolate position embedding", e))
    };

    let output00 = weighted(index00, weight00)?;
    let output01 = weighted(index01, weight01)?;
    let output10 = weighted(index10, weight10)?;
    let output11 = weighted(index11, weight11)?;
    ((&output00 + &output01)
        .and_then(|output| &output + &output10)
        .and_then(|output| &output + &output11))
    .map_err(|e| candle_to_ocr_inference("OvisOCR2", "sum interpolated position embedding", e))
}

fn build_vision_rotary_embeddings(
    rotary_embedding: &VisionRotaryEmbedding,
    grid_thw: (usize, usize, usize),
    merge_size: usize,
    device: &Device,
) -> Result<(Tensor, Tensor), Error> {
    let (_, grid_h, grid_w) = grid_thw;
    let coordinates = merge_grouped_spatial_coordinates(grid_thw, merge_size)?;
    let max_grid = grid_h.max(grid_w);
    let frequency_table = rotary_embedding.forward(max_grid)?;
    let num_patches = coordinates.len();
    let (height_ids, width_ids): (Vec<u32>, Vec<u32>) = coordinates
        .into_iter()
        .map(|(height, width)| (height as u32, width as u32))
        .unzip();
    let height_ids = Tensor::from_vec(height_ids, num_patches, device)
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision height positions", e))?;
    let width_ids = Tensor::from_vec(width_ids, num_patches, device)
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision width positions", e))?;
    let height_frequencies = frequency_table
        .index_select(&height_ids, 0)
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "gather vision height frequencies", e))?;
    let width_frequencies = frequency_table
        .index_select(&width_ids, 0)
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "gather vision width frequencies", e))?;
    let rotary = Tensor::cat(&[&height_frequencies, &width_frequencies], 1)
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "join vision rotary frequencies", e))?;
    let embedding = Tensor::cat(&[&rotary, &rotary], 1)
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "expand vision rotary frequencies", e))?;
    let cos = embedding
        .cos()
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision rotary cos", e))?;
    let sin = embedding
        .sin()
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "vision rotary sin", e))?;
    Ok((cos, sin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn learned_position_interpolation_matches_bilinear_reference() -> Result<(), Error> {
        let weights = Tensor::from_vec(vec![0.0f32, 1.0, 2.0, 3.0], (4, 1), &Device::Cpu)
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "test position tensor", e))?;
        let output = interpolate_position_embedding(&weights, 2, (1, 3, 3), 1)?;
        let values = output
            .flatten_all()
            .and_then(|output| output.to_vec1::<f32>())
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "test position values", e))?;
        let expected = [0.0, 0.5, 1.0, 1.0, 1.5, 2.0, 2.0, 2.5, 3.0];
        for (actual, expected) in values.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
        }
        Ok(())
    }

    #[test]
    fn learned_positions_follow_spatial_merge_group_order() -> Result<(), Error> {
        let weights = Tensor::from_vec(
            (0..16).map(|value| value as f32).collect::<Vec<_>>(),
            (16, 1),
            &Device::Cpu,
        )
        .map_err(|e| candle_to_ocr_inference("OvisOCR2", "test merge position tensor", e))?;
        let output = interpolate_position_embedding(&weights, 4, (1, 4, 4), 2)?;
        let values = output
            .flatten_all()
            .and_then(|output| output.to_vec1::<f32>())
            .map_err(|e| candle_to_ocr_inference("OvisOCR2", "test merge position values", e))?;
        assert_eq!(
            values,
            [
                0.0, 1.0, 4.0, 5.0, 2.0, 3.0, 6.0, 7.0, 8.0, 9.0, 12.0, 13.0, 10.0, 11.0, 14.0,
                15.0,
            ]
        );
        Ok(())
    }

    #[test]
    fn loads_qwen35_weight_names_and_forwards_tiny_tower() -> Result<(), Error> {
        let cfg = OvisOcr2VisionConfig {
            model_type: "qwen3_5".to_string(),
            depth: 0,
            hidden_size: 4,
            intermediate_size: 8,
            num_heads: 1,
            in_channels: 3,
            patch_size: 1,
            spatial_merge_size: 1,
            temporal_patch_size: 1,
            out_hidden_size: 4,
            num_position_embeddings: 4,
            hidden_act: Activation::GeluPytorchTanh,
            initializer_range: 0.02,
            deepstack_visual_indexes: Vec::new(),
        };
        let device = Device::Cpu;
        let mut tensors = HashMap::new();
        tensors.insert(
            "patch_embed.proj.weight".to_string(),
            Tensor::zeros((4, 3, 1, 1, 1), DType::F32, &device).unwrap(),
        );
        tensors.insert(
            "patch_embed.proj.bias".to_string(),
            Tensor::zeros(4, DType::F32, &device).unwrap(),
        );
        tensors.insert(
            "pos_embed.weight".to_string(),
            Tensor::zeros((4, 4), DType::F32, &device).unwrap(),
        );
        tensors.insert(
            "merger.norm.weight".to_string(),
            Tensor::ones(4, DType::F32, &device).unwrap(),
        );
        tensors.insert(
            "merger.norm.bias".to_string(),
            Tensor::zeros(4, DType::F32, &device).unwrap(),
        );
        for name in ["merger.linear_fc1", "merger.linear_fc2"] {
            tensors.insert(
                format!("{name}.weight"),
                Tensor::zeros((4, 4), DType::F32, &device).unwrap(),
            );
            tensors.insert(
                format!("{name}.bias"),
                Tensor::zeros(4, DType::F32, &device).unwrap(),
            );
        }
        let vb = VarBuilder::from_tensors(tensors, DType::F32, &device);
        let model = OvisOcr2VisionModel::load(&cfg, vb)?;
        let output = model.forward(
            &Tensor::zeros((4, 3), DType::F32, &device).unwrap(),
            (1, 2, 2),
        )?;
        assert_eq!(output.dims(), &[4, 4]);
        Ok(())
    }
}
