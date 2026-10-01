//! Vision abstractor (`patch_merger2x`) bridging the Qwen2-VL vision tower to
//! the SDAR text decoder.
//!
//! Mirrors the reference `PatchMerger`: per-patch hidden states (dim
//! `context_dim`) are layer-normed, grouped into `merge_size`×`merge_size`
//! tiles, then projected through a two-layer GELU MLP to the LLM hidden size.
//! With `merge_size = 2` it consumes 4 vision patches per produced token,
//! matching the image-token budget (`prod(grid_thw) / merge_size²`).

use crate::error::Error;
use crate::utils::{candle_to_ocr_inference, candle_to_ocr_processing};
use candle_core::Tensor;
use candle_nn::{LayerNorm, LayerNormConfig, Linear, Module, VarBuilder, layer_norm, linear};

#[derive(Debug)]
pub struct VisionAbstractor {
    ln_q: LayerNorm,
    mlp0: Linear,
    mlp2: Linear,
    /// `context_dim * merge_size²` — the flattened tile width fed to the MLP.
    merged_dim: usize,
}

impl VisionAbstractor {
    /// Load from the `vision_abstractor.projection` prefix.
    ///
    /// * `context_dim` — vision tower output dim (`embed_dim`, 1280).
    /// * `out_dim` — LLM hidden size (2048).
    /// * `merge_size` — spatial merge factor (2 for `patch_merger2x`).
    pub fn load(
        vb: VarBuilder,
        context_dim: usize,
        out_dim: usize,
        merge_size: usize,
    ) -> Result<Self, Error> {
        let norm_cfg = LayerNormConfig {
            eps: 1e-6,
            ..Default::default()
        };
        let ln_q = layer_norm(context_dim, norm_cfg, vb.pp("ln_q"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load abstractor ln_q", e))?;
        let merged_dim = context_dim * merge_size * merge_size;
        let mlp0 = linear(merged_dim, merged_dim, vb.pp("mlp.0"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load abstractor mlp.0", e))?;
        let mlp2 = linear(merged_dim, out_dim, vb.pp("mlp.2"))
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "load abstractor mlp.2", e))?;
        Ok(Self {
            ln_q,
            mlp0,
            mlp2,
            merged_dim,
        })
    }

    /// `x`: per-patch hidden states `(num_patches, context_dim)`.
    /// Returns `(num_patches / merge_size², out_dim)`.
    pub fn forward(&self, x: &Tensor) -> Result<Tensor, Error> {
        let num_patches = x
            .dim(0)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "abstractor dim", e))?;
        let x = self
            .ln_q
            .forward(x)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "abstractor ln_q", e))?;
        // ln_q output is (num_patches, context_dim); regroup into tiles of
        // `merged_dim`. num_patches * context_dim must be divisible by merged_dim.
        let total = num_patches
            * x.dim(1)
                .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "abstractor dim1", e))?;
        if !total.is_multiple_of(self.merged_dim) {
            return Err(Error::InvalidInput {
                message: format!(
                    "MinerU-Diffusion: abstractor element count {total} not divisible by merged_dim {}",
                    self.merged_dim
                ),
            });
        }
        let rows = total / self.merged_dim;
        let x = x.reshape((rows, self.merged_dim)).map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU-Diffusion: abstractor reshape failed",
                e,
            )
        })?;
        let x = self
            .mlp0
            .forward(&x)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "abstractor mlp.0", e))?;
        let x = x.gelu_erf().map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "MinerU-Diffusion: abstractor gelu failed",
                e,
            )
        })?;
        self.mlp2
            .forward(&x)
            .map_err(|e| candle_to_ocr_inference("MinerU-Diffusion", "abstractor mlp.2", e))
    }
}
