use super::config::{PaddleOcrVlConfig, PaddleOcrVlVisionConfig};
use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::{D, Tensor};
use candle_nn::Module;

#[derive(Debug, Clone)]
pub struct Projector {
    pre_norm: candle_nn::LayerNorm,
    linear_1: candle_nn::Linear,
    linear_2: candle_nn::Linear,
    merge_size: usize,
}

impl Projector {
    pub fn load(
        text_cfg: &PaddleOcrVlConfig,
        vision_cfg: &PaddleOcrVlVisionConfig,
        vb: candle_nn::VarBuilder,
    ) -> Result<Self, Error> {
        let ln_cfg = candle_nn::LayerNormConfig {
            eps: 1e-5,
            remove_mean: true,
            affine: true,
        };
        let pre_norm = candle_nn::layer_norm(vision_cfg.hidden_size, ln_cfg, vb.pp("pre_norm"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load projector pre_norm", e))?;
        let hidden_size =
            vision_cfg.hidden_size * vision_cfg.spatial_merge_size * vision_cfg.spatial_merge_size;
        let linear_1 = candle_nn::linear(hidden_size, hidden_size, vb.pp("linear_1"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load projector linear_1", e))?;
        let linear_2 = candle_nn::linear(hidden_size, text_cfg.hidden_size, vb.pp("linear_2"))
            .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "load projector linear_2", e))?;
        Ok(Self {
            pre_norm,
            linear_1,
            linear_2,
            merge_size: vision_cfg.spatial_merge_size,
        })
    }

    pub fn forward(
        &self,
        image_features: &[Tensor],
        image_grid_thw: &[(usize, usize, usize)],
    ) -> Result<Tensor, Error> {
        if image_features.len() != image_grid_thw.len() {
            return Err(Error::InvalidInput {
                message: format!(
                    "PaddleOCR-VL: image_features len {} does not match image_grid_thw len {}",
                    image_features.len(),
                    image_grid_thw.len()
                ),
            });
        }
        let mut projected: Vec<Tensor> = Vec::with_capacity(image_features.len());
        let m = self.merge_size;

        for (feat, &(t, h, w)) in image_features.iter().zip(image_grid_thw.iter()) {
            let feat = self.pre_norm.forward(feat).map_err(|e| {
                candle_to_ocr_inference("PaddleOCR-VL", "projector pre_norm forward", e)
            })?;

            if h % m != 0 || w % m != 0 {
                return Err(Error::InvalidInput {
                    message: format!(
                        "PaddleOCR-VL: image grid {t}x{h}x{w} not divisible by merge_size={m}"
                    ),
                });
            }

            let d = feat.dim(D::Minus1).map_err(|e| {
                candle_to_ocr_processing(
                    crate::error::ProcessingStage::TensorOperation,
                    "PaddleOCR-VL: projector dim failed",
                    e,
                )
            })?;

            let hb = h / m;
            let wb = w / m;
            let feat = feat
                .reshape((t, h, w, d))
                .map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "PaddleOCR-VL: projector reshape thwd failed",
                        e,
                    )
                })?
                .reshape((t, hb, m, wb, m, d))
                .map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "PaddleOCR-VL: projector reshape blocks failed",
                        e,
                    )
                })?
                .permute((0, 1, 3, 2, 4, 5))
                .map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "PaddleOCR-VL: projector permute failed",
                        e,
                    )
                })?
                .reshape((t * hb * wb, m * m * d))
                .map_err(|e| {
                    candle_to_ocr_processing(
                        crate::error::ProcessingStage::TensorOperation,
                        "PaddleOCR-VL: projector merge reshape failed",
                        e,
                    )
                })?;

            let hidden = self.linear_1.forward(&feat).map_err(|e| {
                candle_to_ocr_inference("PaddleOCR-VL", "projector linear_1 forward", e)
            })?;
            let hidden = hidden
                .gelu_erf()
                .map_err(|e| candle_to_ocr_inference("PaddleOCR-VL", "projector gelu_erf", e))?;
            let hidden = self.linear_2.forward(&hidden).map_err(|e| {
                candle_to_ocr_inference("PaddleOCR-VL", "projector linear_2 forward", e)
            })?;

            projected.push(hidden);
        }

        let refs: Vec<&Tensor> = projected.iter().collect();
        Tensor::cat(&refs, 0).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "PaddleOCR-VL: projector concat failed",
                e,
            )
        })
    }
}
