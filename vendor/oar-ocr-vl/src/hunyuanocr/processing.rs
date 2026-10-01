use super::config::{HunyuanOcrImageProcessorConfig, HunyuanOcrVersion, HunyuanOcrVisionConfig};
use crate::error::Error;
use crate::utils::{
    candle_to_ocr_processing,
    image::{clamp_to_max_image_size, image_to_chw, smart_resize},
};
use candle_core::{DType, Device, Tensor};
use image::{RgbImage, imageops::FilterType};

#[derive(Debug, Clone)]
pub struct HunyuanOcrImageInputs {
    pub pixel_values: Tensor,
    pub grid_thw_merged: (usize, usize, usize),
}

pub(crate) fn resized_dimensions(
    height: u32,
    width: u32,
    cfg: &HunyuanOcrImageProcessorConfig,
    vision_cfg: &HunyuanOcrVisionConfig,
    version: HunyuanOcrVersion,
) -> Result<(u32, u32), Error> {
    let factor = (cfg.patch_size * cfg.merge_size) as u32;
    match version {
        HunyuanOcrVersion::V1 => {
            let (rh, rw) = smart_resize_token_limited(
                height,
                width,
                factor,
                cfg.min_pixels,
                cfg.max_pixels,
                vision_cfg.img_max_token_num,
            )?;
            clamp_to_max_image_size(rh, rw, factor, vision_cfg.max_image_size)
        }
        HunyuanOcrVersion::V1_5 => {
            smart_resize(height, width, factor, cfg.min_pixels, cfg.max_pixels)
        }
    }
}

pub fn smart_resize_token_limited(
    height: u32,
    width: u32,
    factor: u32,
    min_pixels: u32,
    max_pixels: u32,
    max_tokens: usize,
) -> Result<(u32, u32), Error> {
    if factor == 0 {
        return Err(Error::InvalidInput {
            message: "HunyuanOCR smart_resize_token_limited: factor must be > 0".to_string(),
        });
    }
    let (mut rh, mut rw) = smart_resize(height, width, factor, min_pixels, max_pixels)?;

    // Token count in HunYuanVL is based on merged grid with an extra newline token per row:
    // tokens = Hm * (Wm + 1)
    loop {
        let hm = (rh / factor) as usize;
        let wm = (rw / factor) as usize;
        let tokens = hm.saturating_mul(wm.saturating_add(1));
        if tokens <= max_tokens {
            break;
        }

        // Reduce the larger axis (in merged-grid units) first to keep aspect ratio roughly intact.
        if wm >= hm {
            if rw <= factor {
                return Err(Error::InvalidInput {
                    message: "HunyuanOCR smart_resize_token_limited: cannot satisfy max_tokens"
                        .to_string(),
                });
            }
            rw -= factor;
        } else {
            if rh <= factor {
                return Err(Error::InvalidInput {
                    message: "HunyuanOCR smart_resize_token_limited: cannot satisfy max_tokens"
                        .to_string(),
                });
            }
            rh -= factor;
        }
    }

    Ok((rh, rw))
}

pub fn preprocess_image(
    image: &RgbImage,
    cfg: &HunyuanOcrImageProcessorConfig,
    vision_cfg: &HunyuanOcrVisionConfig,
    version: HunyuanOcrVersion,
    device: &Device,
    dtype: DType,
) -> Result<HunyuanOcrImageInputs, Error> {
    cfg.validate()?;
    if vision_cfg.patch_size != cfg.patch_size {
        return Err(Error::Config {
            message: format!(
                "HunyuanOCR patch_size mismatch: preprocessor {} != vision_config {}",
                cfg.patch_size, vision_cfg.patch_size
            ),
        });
    }
    if vision_cfg.spatial_merge_size != cfg.merge_size {
        return Err(Error::Config {
            message: format!(
                "HunyuanOCR merge_size mismatch: preprocessor {} != vision_config.spatial_merge_size {}",
                cfg.merge_size, vision_cfg.spatial_merge_size
            ),
        });
    }

    // Match transformers' HunYuanVLImageProcessor._preprocess: it accepts a
    // resample argument but calls PIL `image.resize((w, h))` without passing it.
    // HunyuanOCR therefore ignores cfg.resample and always uses the Pillow
    // default BICUBIC-equivalent path, even when the config says `resample: 1`.
    let resize_filter = FilterType::CatmullRom;

    let (h, w) = (image.height(), image.width());
    let factor = (cfg.patch_size * cfg.merge_size) as u32;
    // V1.5 raises the processor budget to 16M pixels (4K square).
    // `max_image_size` is the learned positional-embedding base grid, not an
    // input side-length cap; the official processor only smart-resizes.
    let (rh, rw) = resized_dimensions(h, w, cfg, vision_cfg, version)?;

    let resized = if rh != h || rw != w {
        image::imageops::resize(image, rw, rh, resize_filter)
    } else {
        image.clone()
    };

    if rh % factor != 0 || rw % factor != 0 {
        return Err(Error::Config {
            message: format!(
                "HunyuanOCR preprocess produced non-divisible dims: {rh}x{rw} not divisible by factor={factor}"
            ),
        });
    }

    let t = 1usize;

    let hm = (rh / factor) as usize;
    let wm = (rw / factor) as usize;
    let grid_thw_merged = (t, hm, wm);

    let data = image_to_chw(&resized, &cfg.image_mean, &cfg.image_std, Some(1.0 / 255.0));

    let pixel_values = Tensor::from_vec(data, (1usize, 3usize, rh as usize, rw as usize), device)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: failed to create pixel_values tensor",
                e,
            )
        })?
        .to_dtype(dtype)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "HunyuanOCR: failed to cast pixel_values dtype",
                e,
            )
        })?;

    Ok(HunyuanOcrImageInputs {
        pixel_values,
        grid_thw_merged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    #[test]
    fn test_v1_preprocess_clamps_to_max_image_size() -> Result<(), Box<dyn std::error::Error>> {
        let cfg = HunyuanOcrImageProcessorConfig {
            min_pixels: 0,
            max_pixels: 100_000_000,
            patch_size: 16,
            resample: None,
            temporal_patch_size: 1,
            merge_size: 2,
            image_mean: vec![0.5, 0.5, 0.5],
            image_std: vec![0.5, 0.5, 0.5],
        };
        let vision_cfg = HunyuanOcrVisionConfig {
            hidden_size: 1,
            intermediate_size: 1,
            num_attention_heads: 1,
            num_hidden_layers: 1,
            num_channels: 3,
            patch_size: 16,
            spatial_merge_size: 2,
            rms_norm_eps: 1e-5,
            hidden_act: "gelu".to_string(),
            add_patchemb_bias: false,
            cat_extra_token: 0,
            max_vit_seq_len: 1,
            max_image_size: 2048,
            img_max_token_num: 100_000,
            interpolate_mode: "bilinear".to_string(),
        };

        let img = RgbImage::new(512, 3000);
        let out = preprocess_image(
            &img,
            &cfg,
            &vision_cfg,
            HunyuanOcrVersion::V1,
            &Device::Cpu,
            DType::F32,
        )?;
        let (_b, _c, rh, rw) = out.pixel_values.dims4()?;
        assert!(rh <= vision_cfg.max_image_size);
        assert!(rw <= vision_cfg.max_image_size);

        let factor = cfg.patch_size * cfg.merge_size;
        assert_eq!(rh % factor, 0);
        assert_eq!(rw % factor, 0);
        assert_eq!(out.grid_thw_merged, (1, rh / factor, rw / factor));
        Ok(())
    }

    #[test]
    fn test_v15_preprocess_allows_large_side() -> Result<(), Box<dyn std::error::Error>> {
        let cfg = HunyuanOcrImageProcessorConfig {
            min_pixels: 0,
            max_pixels: 100_000_000,
            patch_size: 16,
            resample: None,
            temporal_patch_size: 1,
            merge_size: 2,
            image_mean: vec![0.5, 0.5, 0.5],
            image_std: vec![0.5, 0.5, 0.5],
        };
        let vision_cfg = HunyuanOcrVisionConfig {
            hidden_size: 1,
            intermediate_size: 1,
            num_attention_heads: 1,
            num_hidden_layers: 1,
            num_channels: 3,
            patch_size: 16,
            spatial_merge_size: 2,
            rms_norm_eps: 1e-5,
            hidden_act: "gelu".to_string(),
            add_patchemb_bias: false,
            cat_extra_token: 1,
            max_vit_seq_len: 65_536,
            max_image_size: 2048,
            img_max_token_num: 16_384,
            interpolate_mode: "bilinear".to_string(),
        };

        let img = RgbImage::new(512, 3000);
        let out = preprocess_image(
            &img,
            &cfg,
            &vision_cfg,
            HunyuanOcrVersion::V1_5,
            &Device::Cpu,
            DType::F32,
        )?;
        let (_b, _c, rh, rw) = out.pixel_values.dims4()?;
        assert!(rh > vision_cfg.max_image_size || rw > vision_cfg.max_image_size);
        Ok(())
    }
}
