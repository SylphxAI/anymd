use super::config::{HpdParsingConfig, HpdVisionConfig};
use crate::attention::{
    VISION_CHUNKED_ATTN_CHUNK_SIZE, VISION_CHUNKED_ATTN_SEQ_THRESHOLD, chunked_vision_attention,
    flash_attention, scaled_dot_product_attention,
};
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{IndexOp, Tensor};
use candle_nn::{
    LayerNorm, LayerNormConfig, Linear, Module, VarBuilder, layer_norm, linear, linear_no_bias,
};

const MODEL_NAME: &str = "HPD-Parsing";

fn proc_err(stage: &'static str, error: candle_core::Error) -> Error {
    candle_to_ocr_processing(crate::error::ProcessingStage::TensorOperation, stage, error)
}

#[derive(Debug, Clone)]
struct InternAttention {
    qkv: Linear,
    proj: Linear,
    num_heads: usize,
    head_dim: usize,
    scale: f64,
}

impl InternAttention {
    fn load(cfg: &HpdVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let qkv = if cfg.qkv_bias {
            linear(cfg.hidden_size, cfg.hidden_size * 3, vb.pp("qkv"))
        } else {
            linear_no_bias(cfg.hidden_size, cfg.hidden_size * 3, vb.pp("qkv"))
        }
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision qkv", e))?;
        // The upstream InternViT projection always has a bias; qkv_bias applies
        // only to the fused QKV projection.
        let proj = linear(cfg.hidden_size, cfg.hidden_size, vb.pp("proj"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision projection", e))?;
        let head_dim = cfg.hidden_size / cfg.num_attention_heads;
        Ok(Self {
            qkv,
            proj,
            num_heads: cfg.num_attention_heads,
            head_dim,
            scale: 1.0 / (head_dim as f64).sqrt(),
        })
    }

    fn forward(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let (batch, seq_len, _) = hidden
            .dims3()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read vision attention shape", e))?;
        let qkv = self
            .qkv
            .forward(hidden)
            .and_then(|x| x.reshape((batch, seq_len, 3, self.num_heads, self.head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "project vision qkv", e))?;
        let prepare = |index| -> Result<Tensor, Error> {
            qkv.i((.., .., index, .., ..))
                .and_then(|x| x.transpose(1, 2))
                .and_then(|x| x.contiguous())
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "reshape vision qkv", e))
        };
        let q = prepare(0)?;
        let k = prepare(1)?;
        let v = prepare(2)?;
        let attention = match flash_attention(&q, &k, &v, self.scale, false)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "flash vision attention", e))?
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
            .and_then(|x| x.reshape((batch, seq_len, self.num_heads * self.head_dim)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "reshape vision output", e))?;
        self.proj
            .forward(&attention)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision output projection", e))
    }
}

#[derive(Debug, Clone)]
struct InternMlp {
    fc1: Linear,
    fc2: Linear,
}

impl InternMlp {
    fn load(cfg: &HpdVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        Ok(Self {
            fc1: linear(cfg.hidden_size, cfg.intermediate_size, vb.pp("fc1"))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision MLP fc1", e))?,
            fc2: linear(cfg.intermediate_size, cfg.hidden_size, vb.pp("fc2"))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision MLP fc2", e))?,
        })
    }

    fn forward(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let hidden = self
            .fc1
            .forward(hidden)
            .and_then(|x| x.gelu_erf())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision MLP fc1", e))?;
        self.fc2
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision MLP fc2", e))
    }
}

#[derive(Debug, Clone)]
struct InternBlock {
    norm1: LayerNorm,
    norm2: LayerNorm,
    attention: InternAttention,
    mlp: InternMlp,
    ls1: Tensor,
    ls2: Tensor,
}

impl InternBlock {
    fn load(cfg: &HpdVisionConfig, vb: VarBuilder) -> Result<Self, Error> {
        let norm_cfg = LayerNormConfig {
            eps: cfg.layer_norm_eps,
            ..Default::default()
        };
        Ok(Self {
            norm1: layer_norm(cfg.hidden_size, norm_cfg, vb.pp("norm1"))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision norm1", e))?,
            norm2: layer_norm(cfg.hidden_size, norm_cfg, vb.pp("norm2"))
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision norm2", e))?,
            attention: InternAttention::load(cfg, vb.pp("attn"))?,
            mlp: InternMlp::load(cfg, vb.pp("mlp"))?,
            ls1: vb
                .get(cfg.hidden_size, "ls1")
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision ls1", e))?,
            ls2: vb
                .get(cfg.hidden_size, "ls2")
                .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load vision ls2", e))?,
        })
    }

    fn forward(&self, hidden: &Tensor) -> Result<Tensor, Error> {
        let attention = self
            .norm1
            .forward(hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision norm1", e))
            .and_then(|x| self.attention.forward(&x))?
            .broadcast_mul(&self.ls1)
            .map_err(|e| proc_err("HPD vision layer-scale attention", e))?;
        let hidden =
            (hidden + attention).map_err(|e| proc_err("HPD vision attention residual", e))?;
        let mlp = self
            .norm2
            .forward(&hidden)
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision norm2", e))
            .and_then(|x| self.mlp.forward(&x))?
            .broadcast_mul(&self.ls2)
            .map_err(|e| proc_err("HPD vision layer-scale MLP", e))?;
        (hidden + mlp).map_err(|e| proc_err("HPD vision MLP residual", e))
    }
}

#[derive(Debug, Clone)]
pub struct HpdVisionModel {
    patch_weight: Tensor,
    patch_bias: Tensor,
    class_embedding: Tensor,
    position_embedding: Tensor,
    blocks: Vec<InternBlock>,
    projector_norm: LayerNorm,
    projector_fc1: Linear,
    projector_fc2: Linear,
    grid: usize,
    hidden_size: usize,
    downsample: usize,
}

impl HpdVisionModel {
    pub fn load(cfg: &HpdParsingConfig, vb: VarBuilder) -> Result<Self, Error> {
        let vision = &cfg.vision_config;
        let patch_dim = vision.num_channels * vision.patch_size * vision.patch_size;
        let patch_weight = vb
            .get(
                (
                    vision.hidden_size,
                    vision.num_channels,
                    vision.patch_size,
                    vision.patch_size,
                ),
                "vision_model.embeddings.patch_embedding.weight",
            )
            .and_then(|x| x.reshape((vision.hidden_size, patch_dim)))
            .and_then(|x| x.transpose(0, 1))
            .and_then(|x| x.contiguous())
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load patch embedding", e))?;
        let patch_bias = vb
            .get(
                vision.hidden_size,
                "vision_model.embeddings.patch_embedding.bias",
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load patch bias", e))?;
        let class_embedding = vb
            .get(
                (1, 1, vision.hidden_size),
                "vision_model.embeddings.class_embedding",
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load class embedding", e))?;
        let grid = vision.image_size / vision.patch_size;
        let position_embedding = vb
            .get(
                (1, grid * grid + 1, vision.hidden_size),
                "vision_model.embeddings.position_embedding",
            )
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load position embedding", e))?;
        let mut blocks = Vec::with_capacity(vision.num_hidden_layers);
        for index in 0..vision.num_hidden_layers {
            blocks.push(InternBlock::load(
                vision,
                vb.pp(format!("vision_model.encoder.layers.{index}")),
            )?);
        }
        let downsample = (1.0 / cfg.downsample_ratio).round() as usize;
        let projector_input = vision.hidden_size * downsample * downsample;
        let projector_norm = layer_norm(
            projector_input,
            LayerNormConfig {
                eps: 1e-5,
                ..Default::default()
            },
            vb.pp("mlp1.0"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load projector norm", e))?;
        let projector_fc1 = linear(projector_input, cfg.llm_config.hidden_size, vb.pp("mlp1.1"))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load projector fc1", e))?;
        let projector_fc2 = linear(
            cfg.llm_config.hidden_size,
            cfg.llm_config.hidden_size,
            vb.pp("mlp1.3"),
        )
        .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "load projector fc2", e))?;
        Ok(Self {
            patch_weight,
            patch_bias,
            class_embedding,
            position_embedding,
            blocks,
            projector_norm,
            projector_fc1,
            projector_fc2,
            grid,
            hidden_size: vision.hidden_size,
            downsample,
        })
    }

    /// Encode `(tiles, grid², patch_dim)` to `(tiles * image_tokens, llm_hidden)`.
    pub fn forward(&self, patches: &Tensor) -> Result<Tensor, Error> {
        let (tiles, patch_count, patch_width) = patches
            .dims3()
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "read image patch shape", e))?;
        if patch_count != self.grid * self.grid {
            return Err(Error::InvalidInput {
                message: format!(
                    "HPD-Parsing received {patch_count} patches per tile, expected {}",
                    self.grid * self.grid
                ),
            });
        }
        let patch_embeddings = patches
            .reshape((tiles * patch_count, patch_width))
            .and_then(|x| x.matmul(&self.patch_weight))
            .and_then(|x| x.reshape((tiles, patch_count, self.hidden_size)))
            .and_then(|x| x.broadcast_add(&self.patch_bias))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "apply patch embedding", e))?;
        let class = self
            .class_embedding
            .broadcast_as((tiles, 1, self.hidden_size))
            .map_err(|e| proc_err("HPD broadcast class embedding", e))?;
        let mut hidden = Tensor::cat(&[&class, &patch_embeddings], 1)
            .and_then(|x| x.broadcast_add(&self.position_embedding))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "add vision embeddings", e))?;
        for block in &self.blocks {
            hidden = block.forward(&hidden)?;
        }
        let hidden = hidden
            .narrow(1, 1, patch_count)
            .and_then(|x| x.reshape((tiles, self.grid, self.grid, self.hidden_size)))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "reshape vision grid", e))?;
        let reduced = self.grid / self.downsample;
        // Exact InternVL pixel-shuffle v2 ordering.
        let hidden = hidden
            .reshape((
                tiles,
                self.grid,
                reduced,
                self.hidden_size * self.downsample,
            ))
            .and_then(|x| x.transpose(1, 2))
            .and_then(|x| {
                x.reshape((
                    tiles,
                    reduced,
                    reduced,
                    self.hidden_size * self.downsample * self.downsample,
                ))
            })
            .and_then(|x| x.transpose(1, 2))
            .and_then(|x| {
                x.reshape((
                    tiles * reduced * reduced,
                    self.hidden_size * self.downsample * self.downsample,
                ))
            })
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "pixel shuffle", e))?;
        let hidden = self
            .projector_norm
            .forward(&hidden)
            .and_then(|x| self.projector_fc1.forward(&x))
            .and_then(|x| x.gelu_erf())
            .and_then(|x| self.projector_fc2.forward(&x))
            .map_err(|e| candle_to_ocr_inference(MODEL_NAME, "vision projector", e))?;
        Ok(hidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::{DType, Device};
    use candle_nn::VarMap;

    fn attention_config(qkv_bias: bool) -> HpdVisionConfig {
        HpdVisionConfig {
            hidden_size: 8,
            intermediate_size: 16,
            num_hidden_layers: 1,
            num_attention_heads: 2,
            num_channels: 3,
            image_size: 4,
            patch_size: 2,
            layer_norm_eps: 1e-6,
            hidden_act: "gelu".to_string(),
            qkv_bias,
            norm_type: "layer_norm".to_string(),
        }
    }

    #[test]
    fn qkv_bias_config_controls_only_qkv_projection() -> Result<(), Error> {
        for qkv_bias in [false, true] {
            let variables = VarMap::new();
            let vb = VarBuilder::from_varmap(&variables, DType::F32, &Device::Cpu);
            let attention = InternAttention::load(&attention_config(qkv_bias), vb)?;
            assert_eq!(attention.qkv.bias().is_some(), qkv_bias);
            assert!(attention.proj.bias().is_some());
        }
        Ok(())
    }
}
