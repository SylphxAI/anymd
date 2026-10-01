//! PP-DocLayoutV2 reading-order pointer network.
//!
//! Ported from `transformers/models/pp_doclayout_v2/modular_pp_doclayout_v2.py`
//! (`PPDocLayoutV2ReadingOrder`), which is LayoutLMv3's encoder with a
//! box-relation attention bias and a global-pointer head on top.
//!
//! The network sees the surviving detections as a token sequence
//! `[start, pred * n, end, pad ...]` and emits a pairwise "comes before" logit
//! matrix that [`super::order`] turns into a reading sequence.

use super::config::ReadingOrderConfig;
use super::err::infer_err;
use crate::error::Error;
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{
    Conv2d, Conv2dConfig, Embedding, LayerNorm, Linear, Module, VarBuilder, embedding, layer_norm,
    linear,
};

/// Softmax variant from the CogView paper, used verbatim by LayoutLMv3.
fn cogview_softmax(scores: &Tensor) -> Result<Tensor, Error> {
    const ALPHA: f64 = 32.0;
    let scaled = (scores / ALPHA).map_err(|e| infer_err("cogview scale", e))?;
    let max = scaled
        .max_keepdim(D::Minus1)
        .map_err(|e| infer_err("cogview max", e))?;
    let shifted = (scaled
        .broadcast_sub(&max)
        .map_err(|e| infer_err("cogview shift", e))?
        * ALPHA)
        .map_err(|e| infer_err("cogview rescale", e))?;
    candle_nn::ops::softmax_last_dim(&shifted).map_err(|e| infer_err("cogview softmax", e))
}

/// LayoutLM-style token, position and spatial embeddings.
#[derive(Debug)]
struct TextEmbeddings {
    word_embeddings: Embedding,
    token_type_embeddings: Embedding,
    position_embeddings: Embedding,
    x_position_embeddings: Embedding,
    y_position_embeddings: Embedding,
    h_position_embeddings: Embedding,
    w_position_embeddings: Embedding,
    spatial_proj: Linear,
    norm: LayerNorm,
    padding_idx: u32,
    max_2d_position: usize,
}

impl TextEmbeddings {
    fn load(cfg: &ReadingOrderConfig, vb: VarBuilder) -> Result<Self, Error> {
        let hidden = cfg.hidden_size;
        let spatial_dim = 4 * cfg.coordinate_size + 2 * cfg.shape_size;
        Ok(Self {
            word_embeddings: embedding(cfg.vocab_size, hidden, vb.pp("word_embeddings"))
                .map_err(|e| infer_err("load reading-order word embeddings", e))?,
            token_type_embeddings: embedding(
                cfg.type_vocab_size,
                hidden,
                vb.pp("token_type_embeddings"),
            )
            .map_err(|e| infer_err("load reading-order token type embeddings", e))?,
            position_embeddings: embedding(
                cfg.max_position_embeddings,
                hidden,
                vb.pp("position_embeddings"),
            )
            .map_err(|e| infer_err("load reading-order position embeddings", e))?,
            x_position_embeddings: embedding(
                cfg.max_2d_position_embeddings,
                cfg.coordinate_size,
                vb.pp("x_position_embeddings"),
            )
            .map_err(|e| infer_err("load reading-order x embeddings", e))?,
            y_position_embeddings: embedding(
                cfg.max_2d_position_embeddings,
                cfg.coordinate_size,
                vb.pp("y_position_embeddings"),
            )
            .map_err(|e| infer_err("load reading-order y embeddings", e))?,
            h_position_embeddings: embedding(
                cfg.max_2d_position_embeddings,
                cfg.shape_size,
                vb.pp("h_position_embeddings"),
            )
            .map_err(|e| infer_err("load reading-order h embeddings", e))?,
            w_position_embeddings: embedding(
                cfg.max_2d_position_embeddings,
                cfg.shape_size,
                vb.pp("w_position_embeddings"),
            )
            .map_err(|e| infer_err("load reading-order w embeddings", e))?,
            spatial_proj: linear(spatial_dim, hidden, vb.pp("spatial_proj"))
                .map_err(|e| infer_err("load reading-order spatial projection", e))?,
            norm: layer_norm(hidden, cfg.layer_norm_eps, vb.pp("norm"))
                .map_err(|e| infer_err("load reading-order embedding norm", e))?,
            padding_idx: cfg.pad_token_id,
            max_2d_position: cfg.max_2d_position_embeddings,
        })
    }

    /// `input_ids` is `(batch, seq)`, `boxes` is `(batch, seq, 4)` xyxy in the
    /// 0..=1000 range.
    fn forward(
        &self,
        input_ids: &[Vec<u32>],
        boxes: &[Vec<[u32; 4]>],
        device: &Device,
    ) -> Result<Tensor, Error> {
        let batch = input_ids.len();
        let seq = input_ids[0].len();

        let flat_ids: Vec<u32> = input_ids.iter().flatten().copied().collect();
        let ids = Tensor::from_vec(flat_ids, (batch, seq), device)
            .map_err(|e| infer_err("reading-order input ids", e))?;
        let words = self
            .word_embeddings
            .forward(&ids)
            .map_err(|e| infer_err("reading-order word lookup", e))?;

        let position_ids: Vec<u32> = input_ids
            .iter()
            .flat_map(|row| {
                let mut running = 0u32;
                row.iter()
                    .map(|&id| {
                        if id != self.padding_idx {
                            running += 1;
                            running + self.padding_idx
                        } else {
                            self.padding_idx
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let positions = Tensor::from_vec(position_ids, (batch, seq), device)
            .map_err(|e| infer_err("reading-order position ids", e))?;
        let position_embeddings = self
            .position_embeddings
            .forward(&positions)
            .map_err(|e| infer_err("reading-order position lookup", e))?;

        let token_types = Tensor::zeros((batch, seq), DType::U32, device)
            .map_err(|e| infer_err("reading-order token types", e))?;
        let token_type_embeddings = self
            .token_type_embeddings
            .forward(&token_types)
            .map_err(|e| infer_err("reading-order token type lookup", e))?;

        let spatial = self.spatial_embeddings(boxes, batch, seq, device)?;
        let spatial = self
            .spatial_proj
            .forward(&spatial)
            .map_err(|e| infer_err("reading-order spatial projection", e))?;

        let embeddings = (words + token_type_embeddings)
            .and_then(|t| t + position_embeddings)
            .and_then(|t| t + spatial)
            .map_err(|e| infer_err("reading-order embedding sum", e))?;
        Ok(embeddings)
    }

    fn spatial_embeddings(
        &self,
        boxes: &[Vec<[u32; 4]>],
        batch: usize,
        seq: usize,
        device: &Device,
    ) -> Result<Tensor, Error> {
        let limit = (self.max_2d_position - 1) as u32;
        let mut left = Vec::with_capacity(batch * seq);
        let mut upper = Vec::with_capacity(batch * seq);
        let mut right = Vec::with_capacity(batch * seq);
        let mut lower = Vec::with_capacity(batch * seq);
        let mut heights = Vec::with_capacity(batch * seq);
        let mut widths = Vec::with_capacity(batch * seq);
        for row in boxes {
            for b in row {
                let (x0, y0, x1, y1) = (
                    b[0].min(limit),
                    b[1].min(limit),
                    b[2].min(limit),
                    b[3].min(limit),
                );
                left.push(x0);
                upper.push(y0);
                right.push(x1);
                lower.push(y1);
                heights.push(y1.saturating_sub(y0).min(1023));
                widths.push(x1.saturating_sub(x0).min(1023));
            }
        }

        let lookup =
            |values: Vec<u32>, table: &Embedding, what: &'static str| -> Result<Tensor, Error> {
                let ids = Tensor::from_vec(values, (batch, seq), device)
                    .map_err(|e| infer_err(what, e))?;
                table.forward(&ids).map_err(|e| infer_err(what, e))
            };

        let parts = vec![
            lookup(
                left,
                &self.x_position_embeddings,
                "reading-order left embedding",
            )?,
            lookup(
                upper,
                &self.y_position_embeddings,
                "reading-order upper embedding",
            )?,
            lookup(
                right,
                &self.x_position_embeddings,
                "reading-order right embedding",
            )?,
            lookup(
                lower,
                &self.y_position_embeddings,
                "reading-order lower embedding",
            )?,
            lookup(
                heights,
                &self.h_position_embeddings,
                "reading-order height embedding",
            )?,
            lookup(
                widths,
                &self.w_position_embeddings,
                "reading-order width embedding",
            )?,
        ];
        Tensor::cat(&parts, D::Minus1).map_err(|e| infer_err("reading-order spatial concat", e))
    }
}

/// Pairwise box-relation attention bias: a RoPE-style encoding of relative
/// positions and sizes, projected to one bias per attention head.
#[derive(Debug)]
struct PositionRelationEmbedding {
    pos_proj: Conv2d,
    inv_freq: Vec<f64>,
    scale: f64,
}

impl PositionRelationEmbedding {
    fn load(cfg: &ReadingOrderConfig, vb: VarBuilder) -> Result<Self, Error> {
        let dim = cfg.relation_bias_embed_dim;
        let half_dim = dim / 2;
        let inv_freq = (0..dim)
            .step_by(2)
            .map(|i| 1.0 / cfg.relation_bias_theta.powf(i as f64 / half_dim as f64))
            .collect();
        Ok(Self {
            pos_proj: candle_nn::conv2d(
                dim * 4,
                cfg.num_attention_heads,
                1,
                Conv2dConfig::default(),
                vb.pp("pos_proj"),
            )
            .map_err(|e| infer_err("load reading-order relation projection", e))?,
            inv_freq,
            scale: cfg.relation_bias_scale,
        })
    }

    /// `boxes` is `(batch, seq, 4)` in cxcywh form; returns `(batch, heads,
    /// seq, seq)`.
    fn forward(&self, boxes: &[Vec<[f32; 4]>], device: &Device) -> Result<Tensor, Error> {
        const EPSILON: f64 = 1e-5;
        let batch = boxes.len();
        let seq = boxes[0].len();
        let freq_len = self.inv_freq.len();
        let channels = 4 * 2 * freq_len;

        let mut data = vec![0f32; batch * channels * seq * seq];
        for (b, row) in boxes.iter().enumerate() {
            for i in 0..seq {
                let source = row[i];
                for (j, &target) in row.iter().enumerate() {
                    let relative = [
                        ((source[0] - target[0]).abs() as f64 / (source[2] as f64 + EPSILON) + 1.0)
                            .ln(),
                        ((source[1] - target[1]).abs() as f64 / (source[3] as f64 + EPSILON) + 1.0)
                            .ln(),
                        ((source[2] as f64 + EPSILON) / (target[2] as f64 + EPSILON)).ln(),
                        ((source[3] as f64 + EPSILON) / (target[3] as f64 + EPSILON)).ln(),
                    ];
                    for (r, value) in relative.iter().enumerate() {
                        for (k, &freq) in self.inv_freq.iter().enumerate() {
                            let angle = value * self.scale * freq;
                            let sin_channel = r * 2 * freq_len + k;
                            let cos_channel = sin_channel + freq_len;
                            let plane = b * channels * seq * seq + i * seq + j;
                            data[plane + sin_channel * seq * seq] = angle.sin() as f32;
                            data[plane + cos_channel * seq * seq] = angle.cos() as f32;
                        }
                    }
                }
            }
        }

        let embedding = Tensor::from_vec(data, (batch, channels, seq, seq), device)
            .map_err(|e| infer_err("reading-order relation embedding", e))?;
        self.pos_proj
            .forward(&embedding)
            .map_err(|e| infer_err("reading-order relation projection", e))
    }
}

/// One LayoutLMv3-style encoder layer with the relation bias added to the raw
/// attention scores.
#[derive(Debug)]
struct EncoderLayer {
    query: Linear,
    key: Linear,
    value: Linear,
    attention_output: Linear,
    attention_norm: LayerNorm,
    intermediate: Linear,
    output: Linear,
    output_norm: LayerNorm,
    num_heads: usize,
    head_dim: usize,
}

impl EncoderLayer {
    fn load(cfg: &ReadingOrderConfig, vb: VarBuilder) -> Result<Self, Error> {
        let hidden = cfg.hidden_size;
        let attention_vb = vb.pp("attention");
        let self_vb = attention_vb.pp("self");
        let output_vb = attention_vb.pp("output");
        Ok(Self {
            query: linear(hidden, hidden, self_vb.pp("query"))
                .map_err(|e| infer_err("load reading-order query", e))?,
            key: linear(hidden, hidden, self_vb.pp("key"))
                .map_err(|e| infer_err("load reading-order key", e))?,
            value: linear(hidden, hidden, self_vb.pp("value"))
                .map_err(|e| infer_err("load reading-order value", e))?,
            attention_output: linear(hidden, hidden, output_vb.pp("dense"))
                .map_err(|e| infer_err("load reading-order attention output", e))?,
            attention_norm: layer_norm(hidden, cfg.layer_norm_eps, output_vb.pp("norm"))
                .map_err(|e| infer_err("load reading-order attention norm", e))?,
            intermediate: linear(
                hidden,
                cfg.intermediate_size,
                vb.pp("intermediate").pp("dense"),
            )
            .map_err(|e| infer_err("load reading-order intermediate", e))?,
            output: linear(cfg.intermediate_size, hidden, vb.pp("output").pp("dense"))
                .map_err(|e| infer_err("load reading-order output", e))?,
            output_norm: layer_norm(hidden, cfg.layer_norm_eps, vb.pp("output").pp("norm"))
                .map_err(|e| infer_err("load reading-order output norm", e))?,
            num_heads: cfg.num_attention_heads,
            head_dim: hidden / cfg.num_attention_heads,
        })
    }

    fn forward(
        &self,
        hidden: &Tensor,
        attention_mask: &Tensor,
        relation_bias: &Tensor,
    ) -> Result<Tensor, Error> {
        let (batch, seq, _) = hidden
            .dims3()
            .map_err(|e| infer_err("reading-order layer shape", e))?;
        let split = |t: Tensor| -> Result<Tensor, Error> {
            t.reshape((batch, seq, self.num_heads, self.head_dim))
                .and_then(|t| t.transpose(1, 2))
                .and_then(|t| t.contiguous())
                .map_err(|e| infer_err("reading-order head split", e))
        };

        let query = split(
            self.query
                .forward(hidden)
                .map_err(|e| infer_err("reading-order query projection", e))?,
        )?;
        let key = split(
            self.key
                .forward(hidden)
                .map_err(|e| infer_err("reading-order key projection", e))?,
        )?;
        let value = split(
            self.value
                .forward(hidden)
                .map_err(|e| infer_err("reading-order value projection", e))?,
        )?;

        // Upstream scales the query rather than the scores to avoid overflow.
        let scaled_query = (query / (self.head_dim as f64).sqrt())
            .map_err(|e| infer_err("reading-order query scale", e))?;
        let scores = scaled_query
            .matmul(
                &key.transpose(2, 3)
                    .and_then(|t| t.contiguous())
                    .map_err(|e| infer_err("reading-order key transpose", e))?,
            )
            .map_err(|e| infer_err("reading-order scores", e))?;
        // The relation bias is added unscaled, unlike LayoutLMv3.
        let scores = (scores + relation_bias)
            .map_err(|e| infer_err("reading-order relation bias", e))?
            .broadcast_add(attention_mask)
            .map_err(|e| infer_err("reading-order attention mask", e))?;

        let probs = cogview_softmax(&scores)?;
        let context = probs
            .matmul(&value)
            .map_err(|e| infer_err("reading-order context", e))?
            .transpose(1, 2)
            .map_err(|e| infer_err("reading-order context transpose", e))?
            .reshape((batch, seq, self.num_heads * self.head_dim))
            .map_err(|e| infer_err("reading-order context reshape", e))?;

        let attention = self
            .attention_output
            .forward(&context)
            .map_err(|e| infer_err("reading-order attention output", e))?;
        let hidden = self
            .attention_norm
            .forward(
                &(attention + hidden)
                    .map_err(|e| infer_err("reading-order attention residual", e))?,
            )
            .map_err(|e| infer_err("reading-order attention norm", e))?;

        let intermediate = self
            .intermediate
            .forward(&hidden)
            .and_then(|t| t.gelu_erf())
            .map_err(|e| infer_err("reading-order intermediate", e))?;
        let output = self
            .output
            .forward(&intermediate)
            .map_err(|e| infer_err("reading-order output", e))?;
        self.output_norm
            .forward(
                &(output + &hidden).map_err(|e| infer_err("reading-order output residual", e))?,
            )
            .map_err(|e| infer_err("reading-order output norm", e))
    }
}

/// Global pointer head: projects to query/key halves and scores every ordered
/// pair, keeping only the strictly upper triangle.
#[derive(Debug)]
struct GlobalPointer {
    dense: Linear,
    head_size: usize,
    tril_mask: bool,
}

impl GlobalPointer {
    fn load(
        hidden_size: usize,
        head_size: usize,
        tril_mask: bool,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        Ok(Self {
            dense: linear(hidden_size, head_size * 2, vb.pp("dense"))
                .map_err(|e| infer_err("load global pointer", e))?,
            head_size,
            tril_mask,
        })
    }

    fn forward(&self, inputs: &Tensor) -> Result<Tensor, Error> {
        let (batch, seq, _) = inputs
            .dims3()
            .map_err(|e| infer_err("global pointer shape", e))?;
        let projected = self
            .dense
            .forward(inputs)
            .and_then(|t| t.reshape((batch, seq, 2, self.head_size)))
            .map_err(|e| infer_err("global pointer projection", e))?;
        let queries = projected
            .narrow(2, 0, 1)
            .and_then(|t| t.squeeze(2))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("global pointer queries", e))?;
        let keys = projected
            .narrow(2, 1, 1)
            .and_then(|t| t.squeeze(2))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("global pointer keys", e))?;

        let logits = queries
            .matmul(
                &keys
                    .transpose(1, 2)
                    .and_then(|t| t.contiguous())
                    .map_err(|e| infer_err("global pointer key transpose", e))?,
            )
            .map_err(|e| infer_err("global pointer logits", e))?;
        let logits = (logits / (self.head_size as f64).sqrt())
            .map_err(|e| infer_err("global pointer scale", e))?;

        if !self.tril_mask {
            return Ok(logits);
        }
        let keep: Vec<u8> = (0..seq)
            .flat_map(|i| (0..seq).map(move |j| u8::from(i < j)))
            .collect();
        let keep = Tensor::from_vec(keep, (1, seq, seq), logits.device())
            .and_then(|t| t.broadcast_as(logits.shape()))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("global pointer mask", e))?;
        let masked = Tensor::full(-1e4f32, logits.shape(), logits.device())
            .and_then(|t| t.to_dtype(logits.dtype()))
            .map_err(|e| infer_err("global pointer fill", e))?;
        keep.where_cond(&logits, &masked)
            .map_err(|e| infer_err("global pointer masking", e))
    }
}

/// The complete V2 reading-order network.
#[derive(Debug)]
pub(super) struct ReadingOrder {
    embeddings: TextEmbeddings,
    label_embeddings: Embedding,
    label_features_projection: Linear,
    layers: Vec<EncoderLayer>,
    relation_bias: PositionRelationEmbedding,
    relative_head: GlobalPointer,
    config: ReadingOrderConfig,
}

impl ReadingOrder {
    pub(super) fn load(cfg: &ReadingOrderConfig, vb: VarBuilder) -> Result<Self, Error> {
        let encoder_vb = vb.pp("encoder");
        let layers_vb = encoder_vb.pp("layer");
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            layers.push(EncoderLayer::load(cfg, layers_vb.pp(i))?);
        }
        Ok(Self {
            embeddings: TextEmbeddings::load(cfg, vb.pp("embeddings"))?,
            label_embeddings: embedding(
                cfg.num_classes,
                cfg.hidden_size,
                vb.pp("label_embeddings"),
            )
            .map_err(|e| infer_err("load reading-order label embeddings", e))?,
            label_features_projection: linear(
                cfg.hidden_size,
                cfg.hidden_size,
                vb.pp("label_features_projection"),
            )
            .map_err(|e| infer_err("load reading-order label projection", e))?,
            layers,
            relation_bias: PositionRelationEmbedding::load(cfg, encoder_vb.pp("rel_bias_module"))?,
            relative_head: GlobalPointer::load(
                cfg.hidden_size,
                cfg.global_pointer_head_size,
                cfg.tril_mask,
                vb.pp("relative_head"),
            )?,
            config: cfg.clone(),
        })
    }

    /// `boxes` are xyxy on a 0..=1000 grid, `labels` are reading-order
    /// categories, and `num_valid` is how many leading entries are real
    /// detections. Returns `(1, num_boxes, num_boxes)` order logits.
    ///
    /// Upstream truncates the boxes to integers for the embedding lookups but
    /// keeps the fractional values for the relation bias, so both forms are
    /// derived here from the same input.
    pub(super) fn forward(
        &self,
        boxes: &[[f32; 4]],
        labels: &[u32],
        num_valid: usize,
        device: &Device,
    ) -> Result<Tensor, Error> {
        let seq_len = boxes.len();
        let padded_len = seq_len + 2;

        let mut input_ids = vec![self.config.pad_token_id; padded_len];
        input_ids[0] = self.config.start_token_id;
        for slot in input_ids.iter_mut().take(num_valid + 1).skip(1) {
            *slot = self.config.pred_token_id;
        }
        input_ids[num_valid + 1] = self.config.end_token_id;

        let mut padded_boxes = Vec::with_capacity(padded_len);
        padded_boxes.push([0f32; 4]);
        padded_boxes.extend_from_slice(boxes);
        padded_boxes.push([0f32; 4]);
        let integer_boxes: Vec<[u32; 4]> = padded_boxes
            .iter()
            .map(|b| [b[0] as u32, b[1] as u32, b[2] as u32, b[3] as u32])
            .collect();

        let embeddings = self
            .embeddings
            .forward(&[input_ids], &[integer_boxes], device)?;

        let label_ids = Tensor::from_vec(labels.to_vec(), (1, seq_len), device)
            .map_err(|e| infer_err("reading-order labels", e))?;
        let label_projection = self
            .label_embeddings
            .forward(&label_ids)
            .and_then(|t| self.label_features_projection.forward(&t))
            .map_err(|e| infer_err("reading-order label projection", e))?;
        let pad = Tensor::zeros(
            (1, 1, self.config.hidden_size),
            label_projection.dtype(),
            device,
        )
        .map_err(|e| infer_err("reading-order label padding", e))?;
        let label_projection = Tensor::cat(&[&pad, &label_projection, &pad], 1)
            .map_err(|e| infer_err("reading-order label concat", e))?;

        let hidden = (embeddings + label_projection)
            .map_err(|e| infer_err("reading-order embedding merge", e))?;
        let mut hidden = self
            .embeddings
            .norm
            .forward(&hidden)
            .map_err(|e| infer_err("reading-order embedding norm", e))?;

        let mask_row: Vec<f32> = (0..padded_len)
            .map(|i| if i < num_valid + 2 { 0.0 } else { f32::MIN })
            .collect();
        let attention_mask = Tensor::from_vec(mask_row, (1, 1, 1, padded_len), device)
            .map_err(|e| infer_err("reading-order attention mask", e))?;

        let relation_boxes: Vec<[f32; 4]> = padded_boxes
            .iter()
            .map(|b| {
                [
                    (b[0] + b[2]) * 0.5,
                    (b[1] + b[3]) * 0.5,
                    (b[2] - b[0]).max(1e-3),
                    (b[3] - b[1]).max(1e-3),
                ]
            })
            .collect();
        let relation_bias = self.relation_bias.forward(&[relation_boxes], device)?;

        for layer in &self.layers {
            hidden = layer.forward(&hidden, &attention_mask, &relation_bias)?;
        }

        let tokens = hidden
            .narrow(1, 1, seq_len)
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("reading-order token slice", e))?;
        self.relative_head.forward(&tokens)
    }
}
