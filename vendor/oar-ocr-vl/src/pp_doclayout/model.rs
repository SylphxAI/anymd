//! PP-DocLayoutV2 / V3 assembly: backbone, hybrid encoder, query selection,
//! decoder and reading order, plus the public [`PpDocLayout`] entry point.
//!
//! V3 also runs a mask branch: its prototypes set the decoder's initial
//! reference boxes when `mask_enhanced` is on (the upstream default), so it is
//! ported here. The per-layer instance masks it can also produce are not,
//! because every consumer of this detector works with axis-aligned boxes.

use super::config::{PpDocLayoutConfig, PpDocLayoutVersion};
use super::decoder::{Decoder, LevelShape, MlpPredictionHead, SharedHeads, inverse_sigmoid};
use super::encoder::HybridEncoder;
use super::err::{MODEL_NAME, infer_err};
use super::hgnetv2::HGNetV2;
use super::layers::SequentialConvNorm;
use super::mask_head::mask_to_box_coordinate;
use super::order::order_sequence;
use super::processing::{PreprocessedImage, preprocess};
use super::reading_order::ReadingOrder;
use crate::error::Error;
use crate::geometry::BoundingBox;
use crate::layout::LayoutDetectionElement;
use crate::layout::{LayoutDetections, LayoutSource};
use candle_core::{DType, Device, Tensor};
use candle_nn::{LayerNorm, Linear, Module, VarBuilder, layer_norm, linear};
use image::RgbImage;
use std::path::Path;

/// A single detected region before it is turned into a layout element.
#[derive(Debug, Clone)]
pub struct Detection {
    /// Box in original-image pixel coordinates, `[x1, y1, x2, y2]`.
    pub bbox: [f32; 4],
    /// Detection class id.
    pub class_id: usize,
    /// Sigmoid confidence.
    pub score: f32,
}

/// The V3-only order head: a per-layer projection feeding a shared global
/// pointer.
#[derive(Debug)]
struct V3OrderHead {
    norm: LayerNorm,
    order_heads: Vec<Linear>,
    pointer: Linear,
    head_size: usize,
    /// Projects decoder queries onto the mask prototypes. With
    /// `mask_enhanced`, the resulting masks set the decoder's initial boxes.
    mask_query_head: MlpPredictionHead,
    mask_enhanced: bool,
}

impl V3OrderHead {
    fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let order_vb = vb.pp("decoder_order_head");
        let mut order_heads = Vec::with_capacity(cfg.decoder_layers);
        for i in 0..cfg.decoder_layers {
            order_heads.push(
                linear(cfg.d_model, cfg.d_model, order_vb.pp(i))
                    .map_err(|e| infer_err("load v3 order head", e))?,
            );
        }
        Ok(Self {
            norm: layer_norm(cfg.d_model, cfg.layer_norm_eps, vb.pp("decoder_norm"))
                .map_err(|e| infer_err("load v3 decoder norm", e))?,
            order_heads,
            pointer: linear(
                cfg.d_model,
                cfg.global_pointer_head_size * 2,
                vb.pp("decoder_global_pointer").pp("dense"),
            )
            .map_err(|e| infer_err("load v3 global pointer", e))?,
            head_size: cfg.global_pointer_head_size,
            mask_query_head: MlpPredictionHead::load(
                cfg.d_model,
                cfg.d_model,
                cfg.num_prototypes,
                3,
                vb.pp("mask_query_head"),
            )?,
            mask_enhanced: cfg.mask_enhanced,
        })
    }

    /// The layer norm applied to decoder states before the class and order
    /// heads.
    fn norm(&self) -> &LayerNorm {
        &self.norm
    }
}

/// The detector trunk shared by both versions.
#[derive(Debug)]
struct Detector {
    backbone: HGNetV2,
    encoder_input_proj: Vec<SequentialConvNorm>,
    encoder: HybridEncoder,
    decoder_input_proj: Vec<SequentialConvNorm>,
    enc_output_dense: Linear,
    enc_output_norm: LayerNorm,
    enc_score_head: Linear,
    enc_bbox_head: MlpPredictionHead,
    decoder: Decoder,
    num_queries: usize,
}

impl Detector {
    fn load(
        cfg: &PpDocLayoutConfig,
        version: PpDocLayoutVersion,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let vb = vb.pp("model");
        let shared_heads = version == PpDocLayoutVersion::V3;
        let backbone = HGNetV2::load(
            &cfg.backbone_return_idx(),
            cfg.batch_norm_eps,
            vb.pp("backbone"),
        )?;

        let encoder_proj_vb = vb.pp("encoder_input_proj");
        let mut encoder_input_proj = Vec::with_capacity(cfg.encoder_in_channels.len());
        for (i, &in_channels) in cfg.encoder_in_channels.iter().enumerate() {
            encoder_input_proj.push(SequentialConvNorm::load(
                in_channels,
                cfg.encoder_hidden_dim,
                1,
                1,
                cfg.batch_norm_eps,
                encoder_proj_vb.pp(i),
            )?);
        }

        let decoder_proj_vb = vb.pp("decoder_input_proj");
        let mut decoder_input_proj = Vec::with_capacity(cfg.decoder_in_channels.len());
        for (i, &in_channels) in cfg.decoder_in_channels.iter().enumerate() {
            decoder_input_proj.push(SequentialConvNorm::load(
                in_channels,
                cfg.d_model,
                1,
                1,
                cfg.batch_norm_eps,
                decoder_proj_vb.pp(i),
            )?);
        }

        let enc_output_vb = vb.pp("enc_output");
        Ok(Self {
            backbone,
            encoder_input_proj,
            encoder: HybridEncoder::load(cfg, shared_heads, vb.pp("encoder"))?,
            decoder_input_proj,
            enc_output_dense: linear(cfg.d_model, cfg.d_model, enc_output_vb.pp(0))
                .map_err(|e| infer_err("load enc_output dense", e))?,
            enc_output_norm: layer_norm(cfg.d_model, cfg.layer_norm_eps, enc_output_vb.pp(1))
                .map_err(|e| infer_err("load enc_output norm", e))?,
            enc_score_head: linear(cfg.d_model, cfg.num_labels(), vb.pp("enc_score_head"))
                .map_err(|e| infer_err("load enc_score_head", e))?,
            enc_bbox_head: MlpPredictionHead::load(
                cfg.d_model,
                cfg.d_model,
                4,
                3,
                vb.pp("enc_bbox_head"),
            )?,
            decoder: Decoder::load(cfg, shared_heads, vb.pp("decoder"))?,
            num_queries: cfg.num_queries,
        })
    }
}

/// Anchor priors in logit space, plus the validity mask that zeroes out anchors
/// too close to the image border.
fn generate_anchors(shapes: &[LevelShape], device: &Device) -> Result<(Tensor, Tensor), Error> {
    const GRID_SIZE: f32 = 0.05;
    const EPS: f32 = 1e-2;

    let total: usize = shapes.iter().map(|s| s.height * s.width).sum();
    let mut anchors = Vec::with_capacity(total * 4);
    let mut valid = Vec::with_capacity(total);
    for (level, shape) in shapes.iter().enumerate() {
        let wh = GRID_SIZE * 2f32.powi(level as i32);
        for y in 0..shape.height {
            for x in 0..shape.width {
                let cx = (x as f32 + 0.5) / shape.width as f32;
                let cy = (y as f32 + 0.5) / shape.height as f32;
                let coords = [cx, cy, wh, wh];
                let is_valid = coords.iter().all(|&c| c > EPS && c < 1.0 - EPS);
                valid.push(if is_valid { 1.0f32 } else { 0.0 });
                for &c in &coords {
                    anchors.push(if is_valid {
                        (c / (1.0 - c)).ln()
                    } else {
                        f32::MAX
                    });
                }
            }
        }
    }

    let anchors = Tensor::from_vec(anchors, (1, total, 4), device)
        .map_err(|e| infer_err("build anchors", e))?;
    let valid = Tensor::from_vec(valid, (1, total, 1), device)
        .map_err(|e| infer_err("build anchor mask", e))?;
    Ok((anchors, valid))
}

/// Native PP-DocLayoutV2 / V3 layout detector.
#[derive(Debug)]
pub struct PpDocLayout {
    detector: Detector,
    reading_order: Option<ReadingOrder>,
    v3_order: Option<V3OrderHead>,
    config: PpDocLayoutConfig,
    version: PpDocLayoutVersion,
    device: Device,
    dtype: DType,
    score_threshold: f32,
    max_elements: usize,
}

impl PpDocLayout {
    /// Default cap on returned regions, matching `max_elements` in
    /// `oar-ocr-core`'s layout configuration.
    ///
    /// A checkpoint has 300 queries, and `DocParser` runs the VL recognizer
    /// once per region, so an unbounded detector turns a pathological page
    /// into hundreds of generation calls.
    pub const DEFAULT_MAX_ELEMENTS: usize = 100;

    /// Default score threshold for checkpoints without per-class thresholds,
    /// which in practice means PP-DocLayoutV3.
    ///
    /// `0.3` is what `oar-ocr-core`'s ONNX path uses for V3
    /// (`LayoutDetectionConfig::with_pp_doclayoutv3_defaults`), so swapping in
    /// this detector keeps the same regions. It is deliberately lower than the
    /// `0.5` that `transformers`' `post_process_object_detection` defaults to;
    /// V3 scores conservatively and 0.5 drops real regions.
    pub const DEFAULT_SCORE_THRESHOLD: f32 = 0.3;

    /// Loads a checkpoint directory containing `config.json` and
    /// `model.safetensors`.
    pub fn from_dir(dir: impl AsRef<Path>, device: Device) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let config = PpDocLayoutConfig::from_path(dir.join("config.json"))?;
        let version = config.version()?;

        let weights = dir.join("model.safetensors");
        if !weights.exists() {
            return Err(Error::Config {
                message: format!(
                    "{MODEL_NAME}: {} does not contain model.safetensors",
                    dir.display()
                ),
            });
        }
        // The checkpoints are float32 and small enough that keeping full
        // precision costs little, and the detector is sensitive to it.
        let dtype = DType::F32;
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights], dtype, &device)
                .map_err(|e| infer_err("map safetensors", e))?
        };

        let detector = Detector::load(&config, version, vb.clone())?;
        let (reading_order, v3_order) = match version {
            PpDocLayoutVersion::V2 => (
                Some(ReadingOrder::load(
                    &config.reading_order(),
                    vb.pp("reading_order"),
                )?),
                None,
            ),
            PpDocLayoutVersion::V3 => (None, Some(V3OrderHead::load(&config, vb.pp("model"))?)),
        };

        Ok(Self {
            detector,
            reading_order,
            v3_order,
            config,
            version,
            device,
            dtype,
            score_threshold: Self::DEFAULT_SCORE_THRESHOLD,
            max_elements: Self::DEFAULT_MAX_ELEMENTS,
        })
    }

    /// Overrides the score threshold used when the checkpoint has no per-class
    /// thresholds (PP-DocLayoutV3).
    pub fn with_score_threshold(mut self, threshold: f32) -> Self {
        self.score_threshold = threshold;
        self
    }

    /// Caps how many regions [`detect`](Self::detect) returns.
    ///
    /// When more regions clear their threshold, the lowest-scoring ones are
    /// dropped; the survivors keep their reading order. `usize::MAX` disables
    /// the cap.
    pub fn with_max_elements(mut self, max_elements: usize) -> Self {
        self.max_elements = max_elements;
        self
    }

    /// Which generation this instance runs.
    pub fn version(&self) -> PpDocLayoutVersion {
        self.version
    }

    /// The model configuration.
    pub fn config(&self) -> &PpDocLayoutConfig {
        &self.config
    }

    /// Detects layout regions, returning them in predicted reading order.
    pub fn detect(&self, image: &RgbImage) -> Result<Vec<Detection>, Error> {
        let prepared = preprocess(image, &self.device, self.dtype)?;
        let (logits, boxes, hidden) = self.forward_detector(&prepared)?;
        self.postprocess(&logits, &boxes, &hidden, &prepared)
    }

    /// Runs backbone, encoder and decoder, returning last-layer logits, boxes
    /// (cxcywh, normalized) and decoder hidden states.
    fn forward_detector(
        &self,
        prepared: &PreprocessedImage,
    ) -> Result<(Tensor, Tensor, Tensor), Error> {
        let detector = &self.detector;
        let features = detector.backbone.forward(&prepared.pixel_values)?;

        // V3's backbone also returns the stride-4 map, which feeds the mask
        // branch rather than the encoder.
        let skip = features
            .len()
            .saturating_sub(detector.encoder_input_proj.len());
        let x4_feature = (skip > 0).then(|| features[0].clone());
        let projected: Result<Vec<Tensor>, Error> = features[skip..]
            .iter()
            .zip(detector.encoder_input_proj.iter())
            .map(|(feature, proj)| proj.forward(feature))
            .collect();
        let encoded = detector.encoder.forward(&projected?)?;
        let mask_features = match &x4_feature {
            Some(x4) => detector.encoder.mask_features(&encoded, x4)?,
            None => None,
        };

        let mut shapes = Vec::with_capacity(encoded.len());
        let mut flattened = Vec::with_capacity(encoded.len());
        for (level, source) in encoded.iter().enumerate() {
            let source = detector.decoder_input_proj[level].forward(source)?;
            let (_, _, height, width) = source
                .dims4()
                .map_err(|e| infer_err("decoder source shape", e))?;
            shapes.push(LevelShape { height, width });
            flattened.push(
                source
                    .flatten_from(2)
                    .and_then(|t| t.transpose(1, 2))
                    .and_then(|t| t.contiguous())
                    .map_err(|e| infer_err("decoder source flatten", e))?,
            );
        }
        let source =
            Tensor::cat(&flattened, 1).map_err(|e| infer_err("decoder source concat", e))?;

        // Only the encoder head sees the anchor-masked features; the decoder
        // cross-attends to the unmasked ones.
        let (anchors, valid_mask) = generate_anchors(&shapes, &self.device)?;
        let memory = source
            .broadcast_mul(&valid_mask)
            .map_err(|e| infer_err("anchor masking", e))?;
        let output_memory = detector
            .enc_output_dense
            .forward(&memory)
            .and_then(|t| detector.enc_output_norm.forward(&t))
            .map_err(|e| infer_err("encoder output head", e))?;

        let enc_class = detector
            .enc_score_head
            .forward(&output_memory)
            .map_err(|e| infer_err("encoder score head", e))?;
        let enc_coords = detector
            .enc_bbox_head
            .forward(&output_memory)?
            .broadcast_add(&anchors)
            .map_err(|e| infer_err("encoder box head", e))?;

        let topk = self.topk_indices(&enc_class, detector.num_queries)?;
        let index = Tensor::from_vec(topk, (1, detector.num_queries), &self.device)
            .map_err(|e| infer_err("topk index tensor", e))?;

        let reference_points = enc_coords
            .index_select(
                &index
                    .flatten_all()
                    .map_err(|e| infer_err("index flatten", e))?,
                1,
            )
            .map_err(|e| infer_err("gather reference points", e))?;
        let queries = output_memory
            .index_select(
                &index
                    .flatten_all()
                    .map_err(|e| infer_err("index flatten", e))?,
                1,
            )
            .map_err(|e| infer_err("gather queries", e))?;

        // PP-DocLayoutV3 replaces the encoder's initial boxes with tight boxes
        // around the prototype masks of the selected queries.
        let reference_points = match (&self.v3_order, &mask_features) {
            (Some(order), Some(mask_features)) if order.mask_enhanced => {
                let normed = order
                    .norm
                    .forward(&queries)
                    .map_err(|e| infer_err("v3 query norm", e))?;
                let mask_query = order.mask_query_head.forward(&normed)?;
                let (_, prototypes, mask_height, mask_width) = mask_features
                    .dims4()
                    .map_err(|e| infer_err("mask feature shape", e))?;
                let flat = mask_features
                    .reshape((1, prototypes, mask_height * mask_width))
                    .map_err(|e| infer_err("mask feature flatten", e))?;
                let masks = mask_query
                    .matmul(&flat)
                    .and_then(|t| t.reshape((1, detector.num_queries, mask_height, mask_width)))
                    .map_err(|e| infer_err("mask logits", e))?;
                let positive = masks
                    .gt(0f64)
                    .and_then(|t| t.to_dtype(masks.dtype()))
                    .map_err(|e| infer_err("mask threshold", e))?;
                inverse_sigmoid(&mask_to_box_coordinate(&positive)?)?
            }
            _ => reference_points,
        };

        // V3 reuses the encoder heads inside the decoder; V2 has its own.
        let shared = self.v3_order.as_ref().map(|order| SharedHeads {
            class_embed: &detector.enc_score_head,
            bbox_embed: &detector.enc_bbox_head,
            norm: order.norm(),
        });
        let output =
            detector
                .decoder
                .forward(&queries, &source, &reference_points, &shapes, shared)?;
        Ok((
            output.logits,
            output.reference_points,
            output.last_hidden_state,
        ))
    }

    /// Indices of the `k` highest per-position maximum class logits, in
    /// descending score order, matching `torch.topk`.
    fn topk_indices(&self, enc_class: &Tensor, k: usize) -> Result<Vec<u32>, Error> {
        let scores = enc_class
            .max(candle_core::D::Minus1)
            .map_err(|e| infer_err("encoder class max", e))?
            .flatten_all()
            .and_then(|t| t.to_vec1::<f32>())
            .map_err(|e| infer_err("encoder class to host", e))?;
        let mut order: Vec<u32> = (0..scores.len() as u32).collect();
        order.sort_by(|&a, &b| {
            scores[b as usize]
                .total_cmp(&scores[a as usize])
                .then(a.cmp(&b))
        });
        order.truncate(k);
        Ok(order)
    }

    /// Applies thresholds and reading order, mapping boxes back to the
    /// original image size.
    fn postprocess(
        &self,
        logits: &Tensor,
        boxes: &Tensor,
        hidden: &Tensor,
        prepared: &PreprocessedImage,
    ) -> Result<Vec<Detection>, Error> {
        let logit_values = logits
            .flatten_all()
            .and_then(|t| t.to_vec1::<f32>())
            .map_err(|e| infer_err("logits to host", e))?;
        let box_values = boxes
            .flatten_all()
            .and_then(|t| t.to_vec1::<f32>())
            .map_err(|e| infer_err("boxes to host", e))?;

        let selection = self.select(
            &logit_values,
            logits.dim(1).map_err(|e| infer_err("logits shape", e))?,
        )?;
        if selection.num_valid == 0 {
            return Ok(Vec::new());
        }

        let order_logits = self.order_logits(&selection, &box_values, hidden)?;
        let sequence = order_sequence(&order_logits)?;
        self.collect(&selection, &sequence, &logit_values, &box_values, prepared)
    }

    /// Which queries survive thresholding, and in what order they enter the
    /// reading-order stage.
    ///
    /// Upstream instead takes the top `num_queries` over the flattened
    /// `(query, class)` score matrix, which lets one query surface twice under
    /// two labels. Keeping the per-query argmax gives one box per query, which
    /// is what every consumer of this detector expects; the two agree whenever
    /// a query has a single class above its threshold.
    fn select(&self, logits: &[f32], num_queries: usize) -> Result<Selection, Error> {
        let num_labels = self.config.num_labels();
        let thresholds = match self.version {
            PpDocLayoutVersion::V2 => Some(self.config.class_thresholds.as_ref().ok_or_else(
                || Error::Config {
                    message: format!("{MODEL_NAME}: PP-DocLayoutV2 config has no class_thresholds"),
                },
            )?),
            PpDocLayoutVersion::V3 => None,
        };

        let mut best_class = Vec::with_capacity(num_queries);
        let mut kept = Vec::with_capacity(num_queries);
        for q in 0..num_queries {
            let row = &logits[q * num_labels..(q + 1) * num_labels];
            let passes = |class_id: usize, logit: f32| {
                sigmoid(logit) >= thresholds.map_or(self.score_threshold, |t| t[class_id])
            };
            // Thresholds are per class for V2, so the highest-scoring class is
            // not necessarily the one that qualifies: a 0.49 class gated at
            // 0.5 loses to a 0.48 class gated at 0.4. Upstream keeps the
            // latter because it ranks every (query, class) pair, so pick the
            // best class among those that clear their own threshold and only
            // fall back to the plain argmax when none does.
            let eligible = row
                .iter()
                .enumerate()
                .filter(|&(class_id, &logit)| passes(class_id, logit))
                .max_by(|(_, a), (_, b)| a.total_cmp(b));
            match eligible {
                Some((class_id, _)) => {
                    best_class.push(class_id);
                    kept.push(true);
                }
                None => {
                    let (class_id, _) = row
                        .iter()
                        .enumerate()
                        .max_by(|(_, a), (_, b)| a.total_cmp(b))
                        .expect("class logits are never empty");
                    best_class.push(class_id);
                    kept.push(false);
                }
            }
        }

        // Surviving queries move to the front, keeping their relative order.
        let mut permutation: Vec<usize> = (0..num_queries).collect();
        permutation.sort_by_key(|&q| !kept[q]);

        Ok(Selection {
            num_valid: kept.iter().filter(|&&k| k).count(),
            best_class,
            permutation,
        })
    }

    /// Pairwise "comes before" logits over the selected queries.
    fn order_logits(
        &self,
        selection: &Selection,
        boxes: &[f32],
        hidden: &Tensor,
    ) -> Result<Tensor, Error> {
        match (&self.reading_order, &self.v3_order) {
            (Some(reading_order), _) => {
                let class_order =
                    self.config
                        .class_order
                        .as_ref()
                        .ok_or_else(|| Error::Config {
                            message: format!(
                                "{MODEL_NAME}: PP-DocLayoutV2 config has no class_order"
                            ),
                        })?;
                let mut ro_boxes = Vec::with_capacity(selection.permutation.len());
                let mut ro_labels = Vec::with_capacity(selection.permutation.len());
                for (slot, &query) in selection.permutation.iter().enumerate() {
                    let valid = slot < selection.num_valid;
                    // Padded slots carry a zero box and the reading-order
                    // category of class 0, as upstream's zero-fill does.
                    ro_boxes.push(if valid {
                        // The pointer network consumes boxes on a 0..=1000 grid.
                        clamped_corners(&boxes[query * 4..query * 4 + 4], 1000.0)
                    } else {
                        [0.0; 4]
                    });
                    ro_labels.push(if valid {
                        class_order[selection.best_class[query]] as u32
                    } else {
                        class_order[0] as u32
                    });
                }
                reading_order.forward(&ro_boxes, &ro_labels, selection.num_valid, &self.device)
            }
            (None, Some(order)) => self.v3_order_logits(order, hidden),
            (None, None) => Err(Error::Config {
                message: format!("{MODEL_NAME}: no reading-order head was loaded"),
            }),
        }
    }

    fn v3_order_logits(&self, order: &V3OrderHead, hidden: &Tensor) -> Result<Tensor, Error> {
        let normed = order
            .norm
            .forward(hidden)
            .map_err(|e| infer_err("v3 decoder norm", e))?;
        let last = order.order_heads.last().ok_or_else(|| Error::Config {
            message: format!("{MODEL_NAME}: PP-DocLayoutV3 has no order heads"),
        })?;
        let projected = last
            .forward(&normed)
            .map_err(|e| infer_err("v3 order projection", e))?;

        let (batch, seq, _) = projected
            .dims3()
            .map_err(|e| infer_err("v3 order shape", e))?;
        let pair = order
            .pointer
            .forward(&projected)
            .and_then(|t| t.reshape((batch, seq, 2, order.head_size)))
            .map_err(|e| infer_err("v3 global pointer", e))?;
        let queries = pair
            .narrow(2, 0, 1)
            .and_then(|t| t.squeeze(2))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("v3 pointer queries", e))?;
        let keys = pair
            .narrow(2, 1, 1)
            .and_then(|t| t.squeeze(2))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("v3 pointer keys", e))?;
        let logits = queries
            .matmul(
                &keys
                    .transpose(1, 2)
                    .and_then(|t| t.contiguous())
                    .map_err(|e| infer_err("v3 pointer transpose", e))?,
            )
            .map_err(|e| infer_err("v3 pointer logits", e))?;
        let logits = (logits / (order.head_size as f64).sqrt())
            .map_err(|e| infer_err("v3 pointer scale", e))?;

        let keep: Vec<u8> = (0..seq)
            .flat_map(|i| (0..seq).map(move |j| u8::from(i < j)))
            .collect();
        let keep = Tensor::from_vec(keep, (1, seq, seq), logits.device())
            .and_then(|t| t.broadcast_as(logits.shape()))
            .and_then(|t| t.contiguous())
            .map_err(|e| infer_err("v3 pointer mask", e))?;
        let masked = Tensor::full(-1e4f32, logits.shape(), logits.device())
            .and_then(|t| t.to_dtype(logits.dtype()))
            .map_err(|e| infer_err("v3 pointer fill", e))?;
        keep.where_cond(&logits, &masked)
            .map_err(|e| infer_err("v3 pointer masking", e))
    }

    /// Turns surviving queries into detections sorted by reading order.
    fn collect(
        &self,
        selection: &Selection,
        sequence: &[usize],
        logits: &[f32],
        boxes: &[f32],
        prepared: &PreprocessedImage,
    ) -> Result<Vec<Detection>, Error> {
        let num_labels = self.config.num_labels();
        let mut detections: Vec<(usize, Detection)> = Vec::with_capacity(selection.num_valid);
        for (slot, &query) in selection
            .permutation
            .iter()
            .enumerate()
            .take(selection.num_valid)
        {
            let class_id = selection.best_class[query];
            let score = sigmoid(logits[query * num_labels + class_id]);
            let corners = scaled_corners_f32(&boxes[query * 4..query * 4 + 4], 1.0);
            let bbox = [
                corners[0] * prepared.original_width as f32,
                corners[1] * prepared.original_height as f32,
                corners[2] * prepared.original_width as f32,
                corners[3] * prepared.original_height as f32,
            ];
            detections.push((
                self.rank_of(sequence, slot, query),
                Detection {
                    bbox,
                    class_id,
                    score,
                },
            ));
        }
        // Over the cap, drop the least confident regions rather than an
        // arbitrary tail, then restore reading order among the survivors.
        if detections.len() > self.max_elements {
            detections.sort_by(|(_, a), (_, b)| b.score.total_cmp(&a.score));
            detections.truncate(self.max_elements);
        }
        detections.sort_by_key(|(rank, _)| *rank);
        Ok(detections.into_iter().map(|(_, d)| d).collect())
    }

    /// Reading-order rank of one surviving query.
    ///
    /// The two versions index `sequence` differently, because they build the
    /// order logits from different things:
    ///
    /// * V2 feeds the pointer network the survivors-first permutation, so row
    ///   `slot` of the logits is `permutation[slot]`. Upstream matches this by
    ///   permuting `logits` and `pred_boxes` the same way inside `forward`.
    /// * V3 reads the order head straight off the decoder states, which stay in
    ///   query order, so row `query` is the one to look at. Upstream does the
    ///   same with `order_seqs.gather(dim=1, index=index)`.
    ///
    /// Using `slot` for V3 only agrees when the survivors happen to be queries
    /// `0..num_valid`, which is common (queries arrive sorted by encoder score)
    /// but not guaranteed: one query below the threshold shifts every later
    /// element onto a neighbour's rank.
    fn rank_of(&self, sequence: &[usize], slot: usize, query: usize) -> usize {
        match self.version {
            PpDocLayoutVersion::V2 => sequence[slot],
            PpDocLayoutVersion::V3 => sequence[query],
        }
    }
}

/// Which queries survive thresholding, and the order they are fed to the
/// reading-order stage in: survivors first, in query order.
struct Selection {
    /// Number of queries above their threshold.
    num_valid: usize,
    /// Argmax class per query, indexed by query.
    best_class: Vec<usize>,
    /// Queries reordered so the survivors come first.
    permutation: Vec<usize>,
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Converts a normalized cxcywh box to corner form scaled by `scale` and
/// clamped to `[0, scale]`.
fn clamped_corners(box_cxcywh: &[f32], scale: f32) -> [f32; 4] {
    let corners = scaled_corners_f32(box_cxcywh, scale);
    [
        corners[0].clamp(0.0, scale),
        corners[1].clamp(0.0, scale),
        corners[2].clamp(0.0, scale),
        corners[3].clamp(0.0, scale),
    ]
}

fn scaled_corners_f32(box_cxcywh: &[f32], scale: f32) -> [f32; 4] {
    let (cx, cy, w, h) = (box_cxcywh[0], box_cxcywh[1], box_cxcywh[2], box_cxcywh[3]);
    [
        (cx - 0.5 * w) * scale,
        (cy - 0.5 * h) * scale,
        (cx + 0.5 * w) * scale,
        (cy + 0.5 * h) * scale,
    ]
}

impl LayoutSource for PpDocLayout {
    fn detect(&self, image: &RgbImage) -> Result<LayoutDetections, Error> {
        let detections = PpDocLayout::detect(self, image)?;
        let elements = detections
            .into_iter()
            .map(|d| LayoutDetectionElement {
                bbox: BoundingBox::from_coords(d.bbox[0], d.bbox[1], d.bbox[2], d.bbox[3]),
                element_type: self.config.label(d.class_id).to_string(),
                score: d.score,
            })
            .collect();
        // `detect` already returns the regions in predicted reading order.
        Ok(LayoutDetections::new(elements))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::safetensors;

    /// Checks that Metal detections match CPU.
    ///
    /// Ignored by default: it needs a local checkpoint and a page image. Run it
    /// with
    ///
    /// ```text
    /// OAR_PP_DOCLAYOUT_DIR=$PWD/models/PP-DocLayoutV3_safetensors \
    /// OAR_PP_DOCLAYOUT_IMAGE=$PWD/page.png \
    ///     cargo test --release -p oar-ocr-vl --features metal -- --ignored metal_matches_cpu
    /// ```
    ///
    /// Guards issue #177, where a wrong Metal reduction left every box garbage.
    #[cfg(all(feature = "metal", target_os = "macos"))]
    #[test]
    #[ignore = "requires a local checkpoint, a page image and a Metal device"]
    fn metal_matches_cpu() {
        let dir = std::env::var("OAR_PP_DOCLAYOUT_DIR")
            .expect("set OAR_PP_DOCLAYOUT_DIR to a PP-DocLayout checkpoint directory");
        let image_path = std::env::var("OAR_PP_DOCLAYOUT_IMAGE")
            .expect("set OAR_PP_DOCLAYOUT_IMAGE to a page image");
        let image = image::open(&image_path).expect("open image").to_rgb8();

        let cpu = PpDocLayout::from_dir(&dir, Device::Cpu).expect("load on cpu");
        let metal = PpDocLayout::from_dir(&dir, Device::new_metal(0).expect("metal device"))
            .expect("load on metal");
        let expected = cpu.detect(&image).expect("cpu detect");
        let actual = metal.detect(&image).expect("metal detect");

        assert_eq!(
            expected.len(),
            actual.len(),
            "detection count differs between CPU and Metal"
        );
        let mut worst_box = 0f32;
        let mut worst_score = 0f32;
        for (i, (want, got)) in expected.iter().zip(&actual).enumerate() {
            assert_eq!(want.class_id, got.class_id, "class differs at index {i}");
            for axis in 0..4 {
                worst_box = worst_box.max((want.bbox[axis] - got.bbox[axis]).abs());
            }
            worst_score = worst_score.max((want.score - got.score).abs());
        }
        println!(
            "{} detections; worst |dbox| = {worst_box:.4} px, worst |dscore| = {worst_score:.6}",
            expected.len()
        );
        assert!(worst_box < 1.0, "boxes diverge by {worst_box} px");
        assert!(worst_score < 0.01, "scores diverge by {worst_score}");
    }

    /// Compares the ported model against a PyTorch reference dump.
    ///
    /// Ignored by default: it needs a local checkpoint plus a reference file
    /// captured from `transformers`. Run it with
    ///
    /// ```text
    /// OAR_PP_DOCLAYOUT_DIR=$PWD/models/PaddlePaddle/PP-DocLayoutV2 \
    /// OAR_PP_DOCLAYOUT_REF=/tmp/ref_v2.safetensors \
    ///     cargo test --release -p oar-ocr-vl -- --ignored pytorch_reference
    /// ```
    ///
    /// The dump must contain `pixel_values`, `logits`, `pred_boxes` and
    /// `order_logits` from `PPDocLayoutV{2,3}ForObjectDetection`. Per-stage
    /// deviations are printed so a failure points at the offending module.
    ///
    /// One caveat for V2: `torch.argsort` is not stable, so the reference
    /// feeds the pointer network in an arbitrary order. Set `OAR_PP_PERM` to
    /// that order (a comma-separated query list) to compare `order_logits`
    /// against it; without it only the detector outputs are meaningful.
    #[test]
    #[ignore = "requires a local checkpoint and a PyTorch reference dump"]
    fn matches_the_pytorch_reference() {
        let dir = std::env::var("OAR_PP_DOCLAYOUT_DIR")
            .expect("set OAR_PP_DOCLAYOUT_DIR to a PP-DocLayout checkpoint directory");
        let reference_path = std::env::var("OAR_PP_DOCLAYOUT_REF")
            .expect("set OAR_PP_DOCLAYOUT_REF to a PyTorch reference dump");

        let device = Device::Cpu;
        let model = PpDocLayout::from_dir(&dir, device.clone()).expect("load checkpoint");
        let reference = safetensors::load(&reference_path, &device).expect("load reference dump");

        let maxdiff = |a: &Tensor, b: &Tensor| -> f32 {
            (a - b)
                .and_then(|d| d.abs())
                .and_then(|d| d.max_all())
                .and_then(|d| d.to_scalar::<f32>())
                .expect("compare tensors")
        };

        // Stage-by-stage, so a failure says which module drifted.
        let pixel_values = reference["pixel_values"].clone();
        let detector = &model.detector;
        let features = detector.backbone.forward(&pixel_values).expect("backbone");
        let skip = features.len() - detector.encoder_input_proj.len();
        let projected: Vec<Tensor> = features[skip..]
            .iter()
            .zip(detector.encoder_input_proj.iter())
            .map(|(feature, proj)| proj.forward(feature).expect("input projection"))
            .collect();
        for (i, actual) in projected.iter().enumerate() {
            println!(
                "encoder_input_proj.{i}: {}",
                maxdiff(actual, &reference[&format!("encoder_input_proj.{i}")])
            );
        }
        let encoded = detector.encoder.forward(&projected).expect("encoder");
        for (i, actual) in encoded.iter().enumerate() {
            println!(
                "encoder.{i}: {}",
                maxdiff(
                    actual,
                    &reference[&format!("encoder.last_hidden_state.{i}")]
                )
            );
        }

        let prepared = PreprocessedImage {
            pixel_values,
            original_width: 800,
            original_height: 800,
        };
        let (logits, boxes, hidden) = model.forward_detector(&prepared).expect("detector");
        // V2 returns its outputs already permuted, so compare against the raw
        // per-layer stacks, which both versions expose unpermuted.
        let last_layer = |name: &str| -> Tensor {
            let stacked = &reference[name];
            let last = stacked.dim(1).expect("layer dimension") - 1;
            stacked
                .narrow(1, last, 1)
                .and_then(|t| t.squeeze(1))
                .expect("last layer")
        };
        for (name, actual, expected) in [
            ("logits", &logits, last_layer("intermediate_logits")),
            (
                "pred_boxes",
                &boxes,
                last_layer("intermediate_reference_points"),
            ),
        ] {
            let diff = maxdiff(actual, &expected);
            println!("{name}: {diff}");
            assert!(diff < 2e-3, "{name} differs from the reference by {diff}");
        }

        let box_values = boxes.flatten_all().unwrap().to_vec1::<f32>().unwrap();
        let logit_values = logits.flatten_all().unwrap().to_vec1::<f32>().unwrap();
        let mut selection = model.select(&logit_values, 300).expect("selection");
        println!("num_valid: {}", selection.num_valid);
        if let Some(head) = order_permutation_override() {
            let mut rest: Vec<usize> = selection
                .permutation
                .iter()
                .copied()
                .filter(|q| !head.contains(q))
                .collect();
            let mut merged = head;
            merged.append(&mut rest);
            selection.permutation = merged;
        }
        let order_logits = model
            .order_logits(&selection, &box_values, &hidden)
            .expect("order logits");
        let diff = maxdiff(&order_logits, &reference["order_logits"]);
        println!("order_logits: {diff}");
        if model.version == PpDocLayoutVersion::V3 || std::env::var("OAR_PP_PERM").is_ok() {
            assert!(
                diff < 1.0,
                "order_logits differ from the reference by {diff}"
            );
        }
    }

    /// Replays `torch.argsort`'s unstable ordering when `OAR_PP_PERM` is set.
    fn order_permutation_override() -> Option<Vec<usize>> {
        std::env::var("OAR_PP_PERM")
            .ok()
            .filter(|spec| !spec.trim().is_empty())
            .map(|spec| {
                spec.split(',')
                    .map(|entry| entry.trim().parse().expect("query index"))
                    .collect()
            })
    }
}
