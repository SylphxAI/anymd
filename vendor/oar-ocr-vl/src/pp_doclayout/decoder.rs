//! RT-DETR transformer decoder with multi-scale deformable cross-attention.
//!
//! Ported from `transformers/models/rt_detr/modeling_rt_detr.py`
//! (`RTDetrDecoder`, `RTDetrDecoderLayer`, `RTDetrMultiscaleDeformableAttention`
//! and the shared `MultiScaleDeformableAttention` sampler).

use super::config::PpDocLayoutConfig;
use super::err::infer_err;
use super::layers::Activation;
use crate::error::Error;
use candle_core::{DType, Tensor};
use candle_nn::{LayerNorm, Linear, Module, VarBuilder, layer_norm, linear};

/// Spatial size of one feature level.
#[derive(Debug, Clone, Copy)]
pub(super) struct LevelShape {
    pub height: usize,
    pub width: usize,
}

impl LevelShape {
    fn len(&self) -> usize {
        self.height * self.width
    }
}

/// The plain MLP head RT-DETR uses for boxes and query positions: ReLU between
/// every layer but the last.
#[derive(Debug)]
pub(super) struct MlpPredictionHead {
    layers: Vec<Linear>,
}

impl MlpPredictionHead {
    pub(super) fn load(
        input_dim: usize,
        hidden_dim: usize,
        output_dim: usize,
        num_layers: usize,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let layers_vb = vb.pp("layers");
        let mut layers = Vec::with_capacity(num_layers);
        for i in 0..num_layers {
            let in_dim = if i == 0 { input_dim } else { hidden_dim };
            let out_dim = if i + 1 == num_layers {
                output_dim
            } else {
                hidden_dim
            };
            layers.push(
                linear(in_dim, out_dim, layers_vb.pp(i))
                    .map_err(|e| infer_err("load mlp head layer", e))?,
            );
        }
        Ok(Self { layers })
    }

    pub(super) fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let mut hidden = x.clone();
        let last = self.layers.len().saturating_sub(1);
        for (i, layer) in self.layers.iter().enumerate() {
            hidden = layer
                .forward(&hidden)
                .map_err(|e| infer_err("mlp head layer", e))?;
            if i != last {
                hidden = hidden.relu().map_err(|e| infer_err("mlp head relu", e))?;
            }
        }
        Ok(hidden)
    }
}

/// Self-attention over the object queries. Position embeddings are added to
/// queries and keys but not to values.
#[derive(Debug)]
struct QuerySelfAttention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    out_proj: Linear,
    num_heads: usize,
    head_dim: usize,
    scaling: f64,
}

impl QuerySelfAttention {
    fn load(hidden_size: usize, num_heads: usize, vb: VarBuilder) -> Result<Self, Error> {
        let head_dim = hidden_size / num_heads;
        Ok(Self {
            q_proj: linear(hidden_size, hidden_size, vb.pp("q_proj"))
                .map_err(|e| infer_err("load decoder q_proj", e))?,
            k_proj: linear(hidden_size, hidden_size, vb.pp("k_proj"))
                .map_err(|e| infer_err("load decoder k_proj", e))?,
            v_proj: linear(hidden_size, hidden_size, vb.pp("v_proj"))
                .map_err(|e| infer_err("load decoder v_proj", e))?,
            out_proj: linear(hidden_size, hidden_size, vb.pp("out_proj"))
                .map_err(|e| infer_err("load decoder out_proj", e))?,
            num_heads,
            head_dim,
            scaling: (head_dim as f64).powf(-0.5),
        })
    }

    fn forward(&self, hidden: &Tensor, position: &Tensor) -> Result<Tensor, Error> {
        let (batch, seq_len, _) = hidden
            .dims3()
            .map_err(|e| infer_err("decoder attention shape", e))?;
        let query_key_input =
            (hidden + position).map_err(|e| infer_err("decoder query position add", e))?;

        let split = |t: Tensor| -> Result<Tensor, Error> {
            t.reshape((batch, seq_len, self.num_heads, self.head_dim))
                .and_then(|t| t.transpose(1, 2))
                .and_then(|t| t.contiguous())
                .map_err(|e| infer_err("decoder attention reshape", e))
        };

        let query = split(
            self.q_proj
                .forward(&query_key_input)
                .map_err(|e| infer_err("decoder q projection", e))?,
        )?;
        let key = split(
            self.k_proj
                .forward(&query_key_input)
                .map_err(|e| infer_err("decoder k projection", e))?,
        )?;
        let value = split(
            self.v_proj
                .forward(hidden)
                .map_err(|e| infer_err("decoder v projection", e))?,
        )?;

        let scores = (query
            .matmul(
                &key.transpose(2, 3)
                    .and_then(|t| t.contiguous())
                    .map_err(|e| infer_err("decoder key transpose", e))?,
            )
            .map_err(|e| infer_err("decoder attention scores", e))?
            * self.scaling)
            .map_err(|e| infer_err("decoder attention scaling", e))?;
        let probs = candle_nn::ops::softmax_last_dim(&scores)
            .map_err(|e| infer_err("decoder attention softmax", e))?;
        let context = probs
            .matmul(&value)
            .map_err(|e| infer_err("decoder attention context", e))?
            .transpose(1, 2)
            .map_err(|e| infer_err("decoder context transpose", e))?
            .reshape((batch, seq_len, self.num_heads * self.head_dim))
            .map_err(|e| infer_err("decoder context reshape", e))?;
        self.out_proj
            .forward(&context)
            .map_err(|e| infer_err("decoder output projection", e))
    }
}

/// Multi-scale deformable attention: each head samples `n_points` locations per
/// feature level around the query's reference box and blends them with learned
/// weights.
#[derive(Debug)]
struct DeformableAttention {
    sampling_offsets: Linear,
    attention_weights: Linear,
    value_proj: Linear,
    output_proj: Linear,
    d_model: usize,
    num_heads: usize,
    num_levels: usize,
    num_points: usize,
}

impl DeformableAttention {
    fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let num_heads = cfg.decoder_attention_heads;
        if !cfg.d_model.is_multiple_of(num_heads) {
            return Err(Error::Config {
                message: format!(
                    "PP-DocLayout: d_model {} is not divisible by decoder_attention_heads {}",
                    cfg.d_model, num_heads
                ),
            });
        }
        let num_levels = cfg.num_feature_levels;
        let num_points = cfg.decoder_n_points;
        Ok(Self {
            sampling_offsets: linear(
                cfg.d_model,
                num_heads * num_levels * num_points * 2,
                vb.pp("sampling_offsets"),
            )
            .map_err(|e| infer_err("load sampling_offsets", e))?,
            attention_weights: linear(
                cfg.d_model,
                num_heads * num_levels * num_points,
                vb.pp("attention_weights"),
            )
            .map_err(|e| infer_err("load attention_weights", e))?,
            value_proj: linear(cfg.d_model, cfg.d_model, vb.pp("value_proj"))
                .map_err(|e| infer_err("load value_proj", e))?,
            output_proj: linear(cfg.d_model, cfg.d_model, vb.pp("output_proj"))
                .map_err(|e| infer_err("load output_proj", e))?,
            d_model: cfg.d_model,
            num_heads,
            num_levels,
            num_points,
        })
    }

    /// `reference_points` is `(batch, num_queries, 4)` in cxcywh form.
    fn forward(
        &self,
        hidden: &Tensor,
        encoder_hidden: &Tensor,
        position: &Tensor,
        reference_points: &Tensor,
        shapes: &[LevelShape],
    ) -> Result<Tensor, Error> {
        let hidden = (hidden + position).map_err(|e| infer_err("deformable position add", e))?;
        let (batch, num_queries, _) = hidden
            .dims3()
            .map_err(|e| infer_err("deformable query shape", e))?;
        let head_dim = self.d_model / self.num_heads;

        let value = self
            .value_proj
            .forward(encoder_hidden)
            .map_err(|e| infer_err("deformable value projection", e))?;

        let offsets = self
            .sampling_offsets
            .forward(&hidden)
            .and_then(|t| {
                t.reshape((
                    batch,
                    num_queries,
                    self.num_heads,
                    self.num_levels,
                    self.num_points,
                    2,
                ))
            })
            .map_err(|e| infer_err("deformable sampling offsets", e))?;

        let weights = self
            .attention_weights
            .forward(&hidden)
            .and_then(|t| {
                t.reshape((
                    batch,
                    num_queries,
                    self.num_heads,
                    self.num_levels * self.num_points,
                ))
            })
            .map_err(|e| infer_err("deformable attention weights", e))?;
        let weights = candle_nn::ops::softmax_last_dim(&weights)
            .and_then(|t| {
                t.reshape((
                    batch,
                    num_queries,
                    self.num_heads,
                    self.num_levels,
                    self.num_points,
                ))
            })
            .map_err(|e| infer_err("deformable weight softmax", e))?;

        let reference = reference_points
            .reshape((batch, num_queries, 1, 1, 1, 4))
            .map_err(|e| infer_err("deformable reference reshape", e))?;
        let centers = reference
            .narrow(5, 0, 2)
            .map_err(|e| infer_err("deformable reference centers", e))?;
        let sizes = reference
            .narrow(5, 2, 2)
            .map_err(|e| infer_err("deformable reference sizes", e))?;

        let scaled_offsets = ((offsets / self.num_points as f64)
            .map_err(|e| infer_err("deformable offset scale", e))?
            .broadcast_mul(&sizes)
            .map_err(|e| infer_err("deformable offset size", e))?
            * 0.5)
            .map_err(|e| infer_err("deformable offset half", e))?;
        let locations = scaled_offsets
            .broadcast_add(&centers)
            .map_err(|e| infer_err("deformable sampling locations", e))?;

        let sampled = self.sample(&value, &locations, shapes, batch, num_queries, head_dim)?;

        let weights = weights
            .permute((0, 2, 1, 3, 4))
            .and_then(|t| t.contiguous())
            .and_then(|t| {
                t.reshape((
                    batch,
                    self.num_heads,
                    num_queries,
                    self.num_levels,
                    self.num_points,
                    1,
                ))
            })
            .map_err(|e| infer_err("deformable weight permute", e))?;
        // Fold (levels, points) into one axis and reduce a rank-3 view. The
        // natural `sum(4).sum(3)` over the rank-6 product is wrong on Metal:
        // candle's strided reduce mis-indexes any non-final axis of a rank>=5
        // tensor (issue #177).
        let blended = sampled
            .broadcast_mul(&weights)
            .and_then(|t| t.contiguous())
            .and_then(|t| {
                t.reshape((
                    batch * self.num_heads * num_queries,
                    self.num_levels * self.num_points,
                    head_dim,
                ))
            })
            .and_then(|t| t.sum(1))
            .and_then(|t| t.reshape((batch, self.num_heads, num_queries, head_dim)))
            .map_err(|e| infer_err("deformable blend", e))?;

        let output = blended
            .permute((0, 2, 1, 3))
            .and_then(|t| t.contiguous())
            .and_then(|t| t.reshape((batch, num_queries, self.d_model)))
            .map_err(|e| infer_err("deformable output reshape", e))?;
        self.output_proj
            .forward(&output)
            .map_err(|e| infer_err("deformable output projection", e))
    }

    /// Bilinear sampling equivalent to `grid_sample(..., align_corners=False,
    /// padding_mode="zeros")`, returning `(batch, heads, queries, levels,
    /// points, head_dim)`.
    fn sample(
        &self,
        value: &Tensor,
        locations: &Tensor,
        shapes: &[LevelShape],
        batch: usize,
        num_queries: usize,
        head_dim: usize,
    ) -> Result<Tensor, Error> {
        let device = value.device();
        let value = value
            .reshape((batch, (), self.num_heads, head_dim))
            .and_then(|t| t.permute((0, 2, 1, 3)))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("deformable value reshape", e))?;

        let mut level_outputs = Vec::with_capacity(self.num_levels);
        let mut offset = 0usize;
        for (level, shape) in shapes.iter().enumerate() {
            let (height, width) = (shape.height, shape.width);
            let level_value = value
                .narrow(2, offset, shape.len())
                .and_then(|t| t.contiguous())
                .and_then(|t| t.reshape((batch * self.num_heads * shape.len(), head_dim)))
                .map_err(|e| infer_err("deformable level value", e))?;
            offset += shape.len();

            let level_loc = locations
                .narrow(3, level, 1)
                .and_then(|t| t.squeeze(3))
                .and_then(|t| t.permute((0, 2, 1, 3, 4)))
                .and_then(|t| t.contiguous())
                .map_err(|e| infer_err("deformable level locations", e))?;
            let x = level_loc
                .narrow(4, 0, 1)
                .and_then(|t| t.squeeze(4))
                .map_err(|e| infer_err("deformable level x", e))?;
            let y = level_loc
                .narrow(4, 1, 1)
                .and_then(|t| t.squeeze(4))
                .map_err(|e| infer_err("deformable level y", e))?;

            // align_corners=False maps a normalized coordinate to
            // `coord * size - 0.5` in pixel space.
            let gx = ((x * width as f64).map_err(|e| infer_err("deformable x scale", e))? - 0.5)
                .map_err(|e| infer_err("deformable x shift", e))?;
            let gy = ((y * height as f64).map_err(|e| infer_err("deformable y scale", e))? - 0.5)
                .map_err(|e| infer_err("deformable y shift", e))?;
            let x0 = gx.floor().map_err(|e| infer_err("deformable x floor", e))?;
            let y0 = gy.floor().map_err(|e| infer_err("deformable y floor", e))?;
            let wx = (&gx - &x0).map_err(|e| infer_err("deformable x frac", e))?;
            let wy = (&gy - &y0).map_err(|e| infer_err("deformable y frac", e))?;

            let plane = plane_offsets(
                batch,
                self.num_heads,
                shape.len(),
                num_queries,
                self.num_points,
                device,
            )?;

            let mut accumulated: Option<Tensor> = None;
            for (dx, dy) in [(0f64, 0f64), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let xi = (&x0 + dx).map_err(|e| infer_err("deformable corner x", e))?;
                let yi = (&y0 + dy).map_err(|e| infer_err("deformable corner y", e))?;

                let valid = in_range(&xi, width)?
                    .mul(&in_range(&yi, height)?)
                    .map_err(|e| infer_err("deformable corner validity", e))?;

                let xc = clamp_index(&xi, width)?;
                let yc = clamp_index(&yi, height)?;
                let flat =
                    ((yc * width as f64).map_err(|e| infer_err("deformable row offset", e))? + xc)
                        .and_then(|t| t + &plane)
                        .map_err(|e| infer_err("deformable flat index", e))?;
                let flat = flat
                    .flatten_all()
                    .and_then(|t| t.to_dtype(DType::U32))
                    .map_err(|e| infer_err("deformable index cast", e))?;

                let gathered = level_value
                    .index_select(&flat, 0)
                    .and_then(|t| {
                        t.reshape((
                            batch,
                            self.num_heads,
                            num_queries,
                            self.num_points,
                            head_dim,
                        ))
                    })
                    .map_err(|e| infer_err("deformable gather", e))?;

                let corner_x = if dx == 0.0 {
                    (1.0 - &wx).map_err(|e| infer_err("deformable weight x", e))?
                } else {
                    wx.clone()
                };
                let corner_y = if dy == 0.0 {
                    (1.0 - &wy).map_err(|e| infer_err("deformable weight y", e))?
                } else {
                    wy.clone()
                };
                let weight = (corner_x * corner_y)
                    .and_then(|w| w * valid)
                    .and_then(|w| w.unsqueeze(4))
                    .map_err(|e| infer_err("deformable corner weight", e))?;

                let contribution = gathered
                    .broadcast_mul(&weight)
                    .map_err(|e| infer_err("deformable corner blend", e))?;
                accumulated = Some(match accumulated {
                    Some(acc) => (acc + contribution)
                        .map_err(|e| infer_err("deformable corner accumulate", e))?,
                    None => contribution,
                });
            }

            let level_output = accumulated
                .ok_or_else(|| Error::Config {
                    message: "PP-DocLayout: deformable sampling produced no corners".to_string(),
                })?
                .unsqueeze(3)
                .map_err(|e| infer_err("deformable level unsqueeze", e))?;
            level_outputs.push(level_output);
        }

        Tensor::cat(&level_outputs, 3).map_err(|e| infer_err("deformable level concat", e))
    }
}

/// `1.0` where `coord` is a valid index into `[0, size)`, else `0.0`.
fn in_range(coord: &Tensor, size: usize) -> Result<Tensor, Error> {
    let low = coord
        .ge(0f64)
        .map_err(|e| infer_err("deformable range low", e))?;
    let high = coord
        .le((size - 1) as f64)
        .map_err(|e| infer_err("deformable range high", e))?;
    low.mul(&high)
        .and_then(|t| t.to_dtype(coord.dtype()))
        .map_err(|e| infer_err("deformable range mask", e))
}

/// Clamps a coordinate into `[0, size - 1]` so the gather stays in bounds; the
/// validity mask zeroes out whatever the clamp pulled back in.
fn clamp_index(coord: &Tensor, size: usize) -> Result<Tensor, Error> {
    coord
        .clamp(0f64, (size - 1) as f64)
        .map_err(|e| infer_err("deformable clamp", e))
}

/// Per-(batch, head) base offsets into the flattened level buffer, broadcast to
/// `(batch, heads, queries, points)`.
fn plane_offsets(
    batch: usize,
    num_heads: usize,
    level_len: usize,
    num_queries: usize,
    num_points: usize,
    device: &candle_core::Device,
) -> Result<Tensor, Error> {
    let values: Vec<f32> = (0..batch * num_heads)
        .map(|i| (i * level_len) as f32)
        .collect();
    Tensor::from_vec(values, (batch, num_heads, 1, 1), device)
        .and_then(|t| t.broadcast_as((batch, num_heads, num_queries, num_points)))
        .and_then(|t| t.contiguous())
        .map_err(|e| infer_err("deformable plane offsets", e))
}

/// One decoder layer: query self-attention, deformable cross-attention, MLP.
#[derive(Debug)]
struct DecoderLayer {
    self_attn: QuerySelfAttention,
    self_attn_layer_norm: LayerNorm,
    encoder_attn: DeformableAttention,
    encoder_attn_layer_norm: LayerNorm,
    fc1: Linear,
    fc2: Linear,
    final_layer_norm: LayerNorm,
    activation: Activation,
}

impl DecoderLayer {
    fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let hidden = cfg.d_model;
        Ok(Self {
            self_attn: QuerySelfAttention::load(
                hidden,
                cfg.decoder_attention_heads,
                vb.pp("self_attn"),
            )?,
            self_attn_layer_norm: layer_norm(
                hidden,
                cfg.layer_norm_eps,
                vb.pp("self_attn_layer_norm"),
            )
            .map_err(|e| infer_err("load decoder self_attn_layer_norm", e))?,
            encoder_attn: DeformableAttention::load(cfg, vb.pp("encoder_attn"))?,
            encoder_attn_layer_norm: layer_norm(
                hidden,
                cfg.layer_norm_eps,
                vb.pp("encoder_attn_layer_norm"),
            )
            .map_err(|e| infer_err("load decoder encoder_attn_layer_norm", e))?,
            fc1: linear(hidden, cfg.decoder_ffn_dim, vb.pp("fc1"))
                .map_err(|e| infer_err("load decoder fc1", e))?,
            fc2: linear(cfg.decoder_ffn_dim, hidden, vb.pp("fc2"))
                .map_err(|e| infer_err("load decoder fc2", e))?,
            final_layer_norm: layer_norm(hidden, cfg.layer_norm_eps, vb.pp("final_layer_norm"))
                .map_err(|e| infer_err("load decoder final_layer_norm", e))?,
            activation: Activation::parse(&cfg.decoder_activation_function)?,
        })
    }

    fn forward(
        &self,
        hidden: &Tensor,
        position: &Tensor,
        reference_points: &Tensor,
        encoder_hidden: &Tensor,
        shapes: &[LevelShape],
    ) -> Result<Tensor, Error> {
        let attn = self.self_attn.forward(hidden, position)?;
        let hidden = (hidden + attn).map_err(|e| infer_err("decoder self residual", e))?;
        let hidden = self
            .self_attn_layer_norm
            .forward(&hidden)
            .map_err(|e| infer_err("decoder self norm", e))?;

        let cross = self.encoder_attn.forward(
            &hidden,
            encoder_hidden,
            position,
            reference_points,
            shapes,
        )?;
        let hidden = (hidden + cross).map_err(|e| infer_err("decoder cross residual", e))?;
        let hidden = self
            .encoder_attn_layer_norm
            .forward(&hidden)
            .map_err(|e| infer_err("decoder cross norm", e))?;

        let ffn = self
            .fc1
            .forward(&hidden)
            .map_err(|e| infer_err("decoder fc1", e))?;
        let ffn = self.activation.apply(&ffn)?;
        let ffn = self
            .fc2
            .forward(&ffn)
            .map_err(|e| infer_err("decoder fc2", e))?;
        let hidden = (hidden + ffn).map_err(|e| infer_err("decoder ffn residual", e))?;
        self.final_layer_norm
            .forward(&hidden)
            .map_err(|e| infer_err("decoder ffn norm", e))
    }
}

/// Decoder output kept by the detection heads.
pub(super) struct DecoderOutput {
    /// Hidden states of the last layer, `(batch, queries, d_model)`.
    pub last_hidden_state: Tensor,
    /// Class logits of the last layer, `(batch, queries, num_labels)`.
    pub logits: Tensor,
    /// Refined boxes of the last layer in cxcywh form, `(batch, queries, 4)`.
    pub reference_points: Tensor,
}

/// Detection heads driving the per-layer box refinement.
///
/// V2 stores one class and box head per decoder layer. V3 shares a single pair
/// with the encoder head and normalizes the hidden states before classifying,
/// so those heads are supplied by the caller.
#[derive(Debug)]
pub(super) enum DecoderHeads {
    /// One head pair per layer, loaded from `decoder.class_embed` /
    /// `decoder.bbox_embed`.
    PerLayer {
        class_embed: Vec<Linear>,
        bbox_embed: Vec<MlpPredictionHead>,
    },
    /// Heads tied to `enc_score_head` / `enc_bbox_head`, passed in per call.
    Shared,
}

/// Heads and norm a shared-head decoder borrows for one forward pass.
pub(super) struct SharedHeads<'a> {
    pub class_embed: &'a Linear,
    pub bbox_embed: &'a MlpPredictionHead,
    pub norm: &'a LayerNorm,
}

/// The RT-DETR decoder stack with iterative box refinement.
#[derive(Debug)]
pub(super) struct Decoder {
    layers: Vec<DecoderLayer>,
    query_pos_head: MlpPredictionHead,
    heads: DecoderHeads,
}

impl Decoder {
    /// `shared_heads` selects the V3 layout, where the class and box heads are
    /// tied to the encoder head instead of being stored per layer.
    pub(super) fn load(
        cfg: &PpDocLayoutConfig,
        shared_heads: bool,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let layers_vb = vb.pp("layers");
        let mut layers = Vec::with_capacity(cfg.decoder_layers);
        for i in 0..cfg.decoder_layers {
            layers.push(DecoderLayer::load(cfg, layers_vb.pp(i))?);
        }

        let heads = if shared_heads {
            DecoderHeads::Shared
        } else {
            let class_vb = vb.pp("class_embed");
            let bbox_vb = vb.pp("bbox_embed");
            let mut class_embed = Vec::with_capacity(cfg.decoder_layers);
            let mut bbox_embed = Vec::with_capacity(cfg.decoder_layers);
            for i in 0..cfg.decoder_layers {
                class_embed.push(
                    linear(cfg.d_model, cfg.num_labels(), class_vb.pp(i))
                        .map_err(|e| infer_err("load decoder class head", e))?,
                );
                bbox_embed.push(MlpPredictionHead::load(
                    cfg.d_model,
                    cfg.d_model,
                    4,
                    3,
                    bbox_vb.pp(i),
                )?);
            }
            DecoderHeads::PerLayer {
                class_embed,
                bbox_embed,
            }
        };

        Ok(Self {
            layers,
            query_pos_head: MlpPredictionHead::load(
                4,
                2 * cfg.d_model,
                cfg.d_model,
                2,
                vb.pp("query_pos_head"),
            )?,
            heads,
        })
    }

    /// `reference_points` arrives unactivated (logit space), as produced by the
    /// encoder head.
    pub(super) fn forward(
        &self,
        queries: &Tensor,
        encoder_hidden: &Tensor,
        reference_points: &Tensor,
        shapes: &[LevelShape],
        shared: Option<SharedHeads<'_>>,
    ) -> Result<DecoderOutput, Error> {
        let mut hidden = queries.clone();
        let mut reference = candle_nn::ops::sigmoid(reference_points)
            .map_err(|e| infer_err("decoder reference sigmoid", e))?;
        let mut logits = None;

        for (idx, layer) in self.layers.iter().enumerate() {
            let position = self.query_pos_head.forward(&reference)?;
            hidden = layer.forward(&hidden, &position, &reference, encoder_hidden, shapes)?;

            let (corners, class_logits) = match (&self.heads, &shared) {
                (
                    DecoderHeads::PerLayer {
                        class_embed,
                        bbox_embed,
                    },
                    _,
                ) => (
                    bbox_embed[idx].forward(&hidden)?,
                    class_embed[idx]
                        .forward(&hidden)
                        .map_err(|e| infer_err("decoder class head", e))?,
                ),
                (DecoderHeads::Shared, Some(shared)) => {
                    // Boxes come off the raw states, classes off the normalized
                    // ones, matching PP-DocLayoutV3.
                    let normed = shared
                        .norm
                        .forward(&hidden)
                        .map_err(|e| infer_err("decoder shared norm", e))?;
                    (
                        shared.bbox_embed.forward(&hidden)?,
                        shared
                            .class_embed
                            .forward(&normed)
                            .map_err(|e| infer_err("decoder shared class head", e))?,
                    )
                }
                (DecoderHeads::Shared, None) => {
                    return Err(Error::Config {
                        message: "PP-DocLayout: shared decoder heads were not supplied".to_string(),
                    });
                }
            };

            let refined = (corners + inverse_sigmoid(&reference)?)
                .map_err(|e| infer_err("decoder box refinement", e))?;
            reference = candle_nn::ops::sigmoid(&refined)
                .map_err(|e| infer_err("decoder box sigmoid", e))?;
            logits = Some(class_logits);
        }

        let logits = logits.ok_or_else(|| Error::Config {
            message: "PP-DocLayout: decoder_layers must be at least 1".to_string(),
        })?;

        Ok(DecoderOutput {
            last_hidden_state: hidden,
            logits,
            reference_points: reference,
        })
    }
}

/// `log(x / (1 - x))` with the upstream clamping.
pub(super) fn inverse_sigmoid(x: &Tensor) -> Result<Tensor, Error> {
    const EPS: f64 = 1e-5;
    let x = x
        .clamp(0f64, 1f64)
        .map_err(|e| infer_err("inverse sigmoid clamp", e))?;
    let numerator = x
        .clamp(EPS, f64::INFINITY)
        .map_err(|e| infer_err("inverse sigmoid numerator", e))?;
    let denominator = (1.0 - &x)
        .and_then(|t| t.clamp(EPS, f64::INFINITY))
        .map_err(|e| infer_err("inverse sigmoid denominator", e))?;
    (numerator / denominator)
        .and_then(|t| t.log())
        .map_err(|e| infer_err("inverse sigmoid log", e))
}

#[cfg(test)]
mod tests {
    use candle_core::{Device, Tensor};

    /// The folded rank-3 reduction must equal the rank-6 `sum(4).sum(3)` form
    /// it stands in for.
    #[test]
    fn folded_blend_matches_rank6_reduction() -> candle_core::Result<()> {
        let device = Device::Cpu;
        let (batch, heads, queries, levels, points, head_dim) = (1, 8, 12, 3, 4, 32);

        let sampled = Tensor::randn(
            0f32,
            1f32,
            (batch, heads, queries, levels, points, head_dim),
            &device,
        )?;
        let weights = Tensor::randn(
            0f32,
            1f32,
            (batch, heads, queries, levels, points, 1),
            &device,
        )?;

        let want = sampled.broadcast_mul(&weights)?.sum(4)?.sum(3)?;
        let got = sampled
            .broadcast_mul(&weights)?
            .contiguous()?
            .reshape((batch * heads * queries, levels * points, head_dim))?
            .sum(1)?
            .reshape((batch, heads, queries, head_dim))?;

        assert_eq!(want.dims(), got.dims());
        let diff = (want - got)?
            .abs()?
            .flatten_all()?
            .max(0)?
            .to_scalar::<f32>()?;
        assert!(diff < 1e-4, "folded blend diverged by {diff}");
        Ok(())
    }

    /// Fails once candle's Metal rank>=5 reduce agrees with CPU, which is the
    /// signal to drop the fold in `DeformableAttention::forward`.
    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    fn metal_rank5_strided_reduce_is_still_broken() -> candle_core::Result<()> {
        let Ok(metal) = Device::new_metal(0) else {
            return Ok(());
        };
        let dims = (2, 3, 4, 5, 6);
        let cpu_t = Tensor::randn(0f32, 1f32, dims, &Device::Cpu)?;
        let metal_t = cpu_t.to_device(&metal)?;

        let max_diff = |a: &Tensor, b: &Tensor| -> candle_core::Result<f32> {
            (a - b.to_device(&Device::Cpu)?)?
                .abs()?
                .flatten_all()?
                .max(0)?
                .to_scalar::<f32>()
        };

        // Reducing axis 3 of a rank-5 tensor: still broken upstream.
        let broken = max_diff(&cpu_t.sum(3)?, &metal_t.sum(3)?)?;

        // The folded rank-3 equivalent agrees with CPU.
        let fold = |t: &Tensor| -> candle_core::Result<Tensor> {
            t.contiguous()?.reshape((2 * 3 * 4, 5, 6))?.sum(1)
        };
        let folded = max_diff(&fold(&cpu_t)?, &fold(&metal_t)?)?;
        assert!(
            folded < 1e-4,
            "folded rank-3 reduction should match CPU, diverged by {folded}"
        );

        // Asserted, not just logged: cargo swallows output from passing tests,
        // so a note here would never reach anyone. Failing is the point — when
        // candle fixes the kernel this test says so and the fold can go.
        assert!(
            broken >= 1e-4,
            "candle's Metal rank-5 strided reduce now matches CPU (max diff {broken}); \
             the fold in DeformableAttention::forward can be simplified"
        );
        Ok(())
    }
}
