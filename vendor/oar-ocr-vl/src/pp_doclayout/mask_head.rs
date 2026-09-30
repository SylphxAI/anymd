//! PP-DocLayoutV3 mask-feature branch.
//!
//! Ported from `PPDocLayoutV3MaskFeatFPN`, `PPDocLayoutV3ScaleHead` and
//! `PPDocLayoutV3EncoderMaskOutput` in
//! `transformers/models/pp_doclayout_v3/modular_pp_doclayout_v3.py`.
//!
//! The prototypes it produces are not just cosmetic: with `mask_enhanced`
//! (the upstream default) the decoder's initial reference boxes are derived
//! from the predicted masks rather than from the encoder's box head.

use super::config::PpDocLayoutConfig;
use super::err::infer_err;
use super::layers::{Activation, NamedConvNormLayer};
use crate::error::Error;
use candle_core::Tensor;
use candle_nn::{Conv2d, Conv2dConfig, Module, VarBuilder};

/// Brings one feature level up to the finest stride with a conv per doubling.
#[derive(Debug)]
struct ScaleHead {
    layers: Vec<NamedConvNormLayer>,
    upsample: bool,
}

impl ScaleHead {
    fn load(
        in_channels: usize,
        feature_channels: usize,
        fpn_stride: usize,
        base_stride: usize,
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        let ratio = (fpn_stride as f64 / base_stride as f64).log2().round() as i64;
        let head_length = ratio.max(1) as usize;
        let upsample = fpn_stride != base_stride;

        let layers_vb = vb.pp("layers");
        let mut layers = Vec::with_capacity(head_length);
        // Upstream interleaves `Upsample` modules into the same `ModuleList`,
        // but only the convolutions carry parameters, so the checkpoint index
        // advances by two whenever an upsample follows.
        let stride_between = if upsample { 2 } else { 1 };
        for k in 0..head_length {
            let in_c = if k == 0 {
                in_channels
            } else {
                feature_channels
            };
            layers.push(NamedConvNormLayer::load(
                in_c,
                feature_channels,
                3,
                1,
                Activation::Silu,
                batch_norm_eps,
                layers_vb.pp(k * stride_between),
            )?);
        }
        Ok(Self { layers, upsample })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let mut hidden = x.clone();
        for layer in &self.layers {
            hidden = layer.forward(&hidden)?;
            if self.upsample {
                let (_, _, height, width) = hidden
                    .dims4()
                    .map_err(|e| infer_err("mask scale head shape", e))?;
                hidden = hidden
                    .upsample_bilinear2d(height * 2, width * 2, false)
                    .map_err(|e| infer_err("mask scale head upsample", e))?;
            }
        }
        Ok(hidden)
    }
}

/// Fuses the PAN levels into a single stride-8 feature map.
#[derive(Debug)]
struct MaskFeatFpn {
    scale_heads: Vec<ScaleHead>,
    output_conv: NamedConvNormLayer,
    order: Vec<usize>,
}

impl MaskFeatFpn {
    fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let strides = cfg.strides();
        let feature_channels = *cfg
            .mask_feature_channels
            .first()
            .ok_or_else(|| Error::Config {
                message: "PP-DocLayoutV3: mask_feature_channels must have two entries".to_string(),
            })?;
        let out_channels = *cfg
            .mask_feature_channels
            .get(1)
            .ok_or_else(|| Error::Config {
                message: "PP-DocLayoutV3: mask_feature_channels must have two entries".to_string(),
            })?;

        let mut order: Vec<usize> = (0..strides.len()).collect();
        order.sort_by_key(|&i| strides[i]);
        let base_stride = strides[order[0]];

        let heads_vb = vb.pp("scale_heads");
        let mut scale_heads = Vec::with_capacity(order.len());
        for (slot, &level) in order.iter().enumerate() {
            scale_heads.push(ScaleHead::load(
                cfg.encoder_hidden_dim,
                feature_channels,
                strides[level],
                base_stride,
                cfg.batch_norm_eps,
                heads_vb.pp(slot),
            )?);
        }

        Ok(Self {
            scale_heads,
            output_conv: NamedConvNormLayer::load(
                feature_channels,
                out_channels,
                3,
                1,
                Activation::Silu,
                cfg.batch_norm_eps,
                vb.pp("output_conv"),
            )?,
            order,
        })
    }

    fn forward(&self, features: &[Tensor]) -> Result<Tensor, Error> {
        let mut output = self.scale_heads[0].forward(&features[self.order[0]])?;
        let (_, _, height, width) = output.dims4().map_err(|e| infer_err("mask fpn shape", e))?;
        for (slot, &level) in self.order.iter().enumerate().skip(1) {
            let scaled = self.scale_heads[slot].forward(&features[level])?;
            let scaled = scaled
                .upsample_bilinear2d(height, width, false)
                .map_err(|e| infer_err("mask fpn resize", e))?;
            output = (output + scaled).map_err(|e| infer_err("mask fpn merge", e))?;
        }
        self.output_conv.forward(&output)
    }
}

/// Final projection to mask prototypes.
#[derive(Debug)]
struct MaskOutput {
    base_conv: NamedConvNormLayer,
    conv: Conv2d,
}

impl MaskOutput {
    fn load(
        in_channels: usize,
        num_prototypes: usize,
        batch_norm_eps: f64,
        vb: VarBuilder,
    ) -> Result<Self, Error> {
        Ok(Self {
            base_conv: NamedConvNormLayer::load(
                in_channels,
                in_channels,
                3,
                1,
                Activation::Silu,
                batch_norm_eps,
                vb.pp("base_conv"),
            )?,
            conv: candle_nn::conv2d(
                in_channels,
                num_prototypes,
                1,
                Conv2dConfig::default(),
                vb.pp("conv"),
            )
            .map_err(|e| infer_err("load mask output convolution", e))?,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let x = self.base_conv.forward(x)?;
        self.conv
            .forward(&x)
            .map_err(|e| infer_err("mask output convolution", e))
    }
}

/// The complete V3 mask branch: FPN over the PAN levels, a stride-4 lateral
/// shortcut, and the prototype projection.
#[derive(Debug)]
pub(super) struct MaskFeatureHead {
    fpn: MaskFeatFpn,
    lateral: NamedConvNormLayer,
    output: MaskOutput,
}

impl MaskFeatureHead {
    pub(super) fn load(cfg: &PpDocLayoutConfig, vb: VarBuilder) -> Result<Self, Error> {
        let out_channels = *cfg
            .mask_feature_channels
            .get(1)
            .ok_or_else(|| Error::Config {
                message: "PP-DocLayoutV3: mask_feature_channels must have two entries".to_string(),
            })?;
        Ok(Self {
            fpn: MaskFeatFpn::load(cfg, vb.pp("mask_feature_head"))?,
            lateral: NamedConvNormLayer::load(
                cfg.x4_feat_dim,
                out_channels,
                3,
                1,
                Activation::Silu,
                cfg.batch_norm_eps,
                vb.pp("encoder_mask_lateral"),
            )?,
            output: MaskOutput::load(
                out_channels,
                cfg.num_prototypes,
                cfg.batch_norm_eps,
                vb.pp("encoder_mask_output"),
            )?,
        })
    }

    /// `pan` are the encoder's output levels, `x4` the stride-4 backbone map.
    pub(super) fn forward(&self, pan: &[Tensor], x4: &Tensor) -> Result<Tensor, Error> {
        let fused = self.fpn.forward(pan)?;
        let (_, _, height, width) = fused
            .dims4()
            .map_err(|e| infer_err("mask feature shape", e))?;
        let upsampled = fused
            .upsample_bilinear2d(height * 2, width * 2, false)
            .map_err(|e| infer_err("mask feature upsample", e))?;
        let lateral = self.lateral.forward(x4)?;
        let merged = (upsampled + lateral).map_err(|e| infer_err("mask feature merge", e))?;
        self.output.forward(&merged)
    }
}

/// Tight boxes around each positive mask, in normalized cxcywh form.
///
/// Empty masks collapse to an all-zero box, matching
/// `mask_to_box_coordinate` upstream.
pub(super) fn mask_to_box_coordinate(masks: &Tensor) -> Result<Tensor, Error> {
    let (batch, count, height, width) =
        masks.dims4().map_err(|e| infer_err("mask box shape", e))?;
    let values = masks
        .flatten_all()
        .and_then(|t| t.to_vec1::<f32>())
        .map_err(|e| infer_err("mask box to host", e))?;

    let mut boxes = Vec::with_capacity(batch * count * 4);
    for plane in 0..batch * count {
        let mask = &values[plane * height * width..(plane + 1) * height * width];
        let mut x_min = usize::MAX;
        let mut y_min = usize::MAX;
        let mut x_max = 0usize;
        let mut y_max = 0usize;
        let mut any = false;
        for y in 0..height {
            for x in 0..width {
                if mask[y * width + x] > 0.0 {
                    any = true;
                    x_min = x_min.min(x);
                    y_min = y_min.min(y);
                    x_max = x_max.max(x);
                    y_max = y_max.max(y);
                }
            }
        }
        if !any {
            boxes.extend_from_slice(&[0.0; 4]);
            continue;
        }
        // Upstream takes the exclusive maximum, hence the `+ 1`.
        let x0 = x_min as f32 / width as f32;
        let y0 = y_min as f32 / height as f32;
        let x1 = (x_max + 1) as f32 / width as f32;
        let y1 = (y_max + 1) as f32 / height as f32;
        boxes.extend_from_slice(&[(x0 + x1) * 0.5, (y0 + y1) * 0.5, x1 - x0, y1 - y0]);
    }

    Tensor::from_vec(boxes, (batch, count, 4), masks.device())
        .map_err(|e| infer_err("mask box tensor", e))
}
