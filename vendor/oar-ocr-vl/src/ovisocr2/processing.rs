use super::config::{OvisOcr2ImageProcessorConfig, OvisOcr2VisionConfig};
use crate::error::Error;
use crate::utils::{
    candle_to_ocr_processing,
    image::{image_to_chw, patchify_merge_grouped, pil_resample_to_filter_type, smart_resize},
};
use candle_core::{DType, Device, Tensor};
use image::{RgbImage, imageops::FilterType};

#[derive(Debug, Clone)]
pub struct OvisOcr2ImageInputs {
    pub pixel_values: Tensor,
    pub grid_thw: (usize, usize, usize),
    pub num_image_tokens: usize,
}

pub(crate) fn validate_processor_vision_compatibility(
    cfg: &OvisOcr2ImageProcessorConfig,
    vision_cfg: &OvisOcr2VisionConfig,
) -> Result<(), Error> {
    if cfg.patch_size != vision_cfg.patch_size {
        return Err(Error::Config {
            message: format!(
                "OvisOCR2 patch_size mismatch: processor {} != vision {}",
                cfg.patch_size, vision_cfg.patch_size
            ),
        });
    }
    if cfg.temporal_patch_size != vision_cfg.temporal_patch_size {
        return Err(Error::Config {
            message: format!(
                "OvisOCR2 temporal_patch_size mismatch: processor {} != vision {}",
                cfg.temporal_patch_size, vision_cfg.temporal_patch_size
            ),
        });
    }
    if cfg.merge_size != vision_cfg.spatial_merge_size {
        return Err(Error::Config {
            message: format!(
                "OvisOCR2 merge_size mismatch: processor {} != vision {}",
                cfg.merge_size, vision_cfg.spatial_merge_size
            ),
        });
    }
    if vision_cfg.in_channels != 3 {
        return Err(Error::Config {
            message: format!(
                "OvisOCR2 image preprocessing supports three RGB channels, got {}",
                vision_cfg.in_channels
            ),
        });
    }
    Ok(())
}

pub fn preprocess_image(
    image: &RgbImage,
    cfg: &OvisOcr2ImageProcessorConfig,
    vision_cfg: &OvisOcr2VisionConfig,
    device: &Device,
    dtype: DType,
) -> Result<OvisOcr2ImageInputs, Error> {
    cfg.validate()?;
    vision_cfg.validate()?;
    validate_processor_vision_compatibility(cfg, vision_cfg)?;

    let factor = cfg
        .patch_size
        .checked_mul(cfg.merge_size)
        .and_then(|factor| u32::try_from(factor).ok())
        .ok_or_else(|| Error::Config {
            message: "OvisOCR2 patch/merge factor overflow".to_string(),
        })?;
    let (min_pixels, max_pixels) = cfg.runtime_pixel_bounds();
    let (height, width) = (image.height(), image.width());
    if height == 0 || width == 0 {
        return Err(Error::InvalidInput {
            message: format!("OvisOCR2 input image must be non-empty, got {width}x{height}"),
        });
    }
    let (resized_height, resized_width) = if cfg.do_resize {
        smart_resize(height, width, factor, min_pixels, max_pixels)?
    } else {
        (height, width)
    };

    if resized_height % factor != 0 || resized_width % factor != 0 {
        return Err(Error::InvalidInput {
            message: format!(
                "OvisOCR2 preprocessed dimensions {resized_height}x{resized_width} must be divisible by {factor}"
            ),
        });
    }

    // Qwen's fast image processor defaults to bicubic when the checkpoint
    // does not include an explicit PIL resampling enum.
    let resize_filter = cfg
        .resample
        .and_then(pil_resample_to_filter_type)
        .unwrap_or(FilterType::CatmullRom);
    let resized = if resized_height != height || resized_width != width {
        image::imageops::resize(image, resized_width, resized_height, resize_filter)
    } else {
        image.clone()
    };

    let default_mean = [0.0_f32; 3];
    let default_std = [1.0_f32; 3];
    let mean = if cfg.do_normalize {
        cfg.image_mean.as_slice()
    } else {
        &default_mean
    };
    let std = if cfg.do_normalize {
        cfg.image_std.as_slice()
    } else {
        &default_std
    };
    let rescale_factor = cfg.do_rescale.then_some(cfg.rescale_factor);
    let frame = image_to_chw(&resized, mean, std, rescale_factor);

    // The image processor represents a still image as one temporal grid cell
    // whose frame is repeated to fill the Conv3D temporal kernel.
    let frames: Vec<&[f32]> =
        std::iter::repeat_n(frame.as_slice(), cfg.temporal_patch_size).collect();
    let grid_t = 1usize;
    let grid_h = resized_height as usize / cfg.patch_size;
    let grid_w = resized_width as usize / cfg.patch_size;
    if !grid_h.is_multiple_of(cfg.merge_size) || !grid_w.is_multiple_of(cfg.merge_size) {
        return Err(Error::Config {
            message: format!(
                "OvisOCR2 patch grid {grid_h}x{grid_w} must be divisible by merge_size {}",
                cfg.merge_size
            ),
        });
    }

    let flat = patchify_merge_grouped(
        &frames,
        vision_cfg.in_channels,
        resized_height as usize,
        resized_width as usize,
        grid_t,
        grid_h,
        grid_w,
        cfg.patch_size,
        cfg.merge_size,
        cfg.temporal_patch_size,
    );
    let patch_dim =
        vision_cfg.in_channels * cfg.temporal_patch_size * cfg.patch_size * cfg.patch_size;
    let num_patches = grid_t * grid_h * grid_w;
    if flat.len() != num_patches * patch_dim {
        return Err(Error::InvalidInput {
            message: format!(
                "OvisOCR2 patch extraction produced {} values, expected {}",
                flat.len(),
                num_patches * patch_dim
            ),
        });
    }

    let pixel_values = Tensor::from_vec(flat, (num_patches, patch_dim), device)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "OvisOCR2: create pixel_values",
                e,
            )
        })?
        .to_dtype(dtype)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "OvisOCR2: cast pixel_values",
                e,
            )
        })?;
    let num_image_tokens = num_patches / (cfg.merge_size * cfg.merge_size);

    Ok(OvisOcr2ImageInputs {
        pixel_values,
        grid_thw: (grid_t, grid_h, grid_w),
        num_image_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ovisocr2::config::OvisOcr2ImageProcessorSize;
    use candle_nn::Activation;
    use image::Rgb;

    fn processor_config() -> OvisOcr2ImageProcessorConfig {
        OvisOcr2ImageProcessorConfig {
            size: OvisOcr2ImageProcessorSize {
                shortest_edge: 256 * 256,
                longest_edge: 4096 * 4096,
            },
            patch_size: 16,
            temporal_patch_size: 2,
            merge_size: 2,
            image_mean: vec![0.5; 3],
            image_std: vec![0.5; 3],
            do_resize: true,
            do_rescale: true,
            do_normalize: true,
            do_convert_rgb: true,
            rescale_factor: 1.0 / 255.0,
            resample: None,
            processor_class: None,
            image_processor_type: None,
        }
    }

    fn vision_config() -> OvisOcr2VisionConfig {
        OvisOcr2VisionConfig {
            model_type: "qwen3_5".to_string(),
            depth: 12,
            hidden_size: 768,
            intermediate_size: 3072,
            num_heads: 12,
            in_channels: 3,
            patch_size: 16,
            spatial_merge_size: 2,
            temporal_patch_size: 2,
            out_hidden_size: 1024,
            num_position_embeddings: 2304,
            hidden_act: Activation::GeluPytorchTanh,
            initializer_range: 0.02,
            deepstack_visual_indexes: Vec::new(),
        }
    }

    #[test]
    fn preprocess_uses_official_minimum_area_and_merge_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let image = RgbImage::from_pixel(32, 32, Rgb([255, 255, 255]));
        let inputs = preprocess_image(
            &image,
            &processor_config(),
            &vision_config(),
            &Device::Cpu,
            DType::F32,
        )?;

        assert_eq!(inputs.grid_thw, (1, 28, 28));
        assert_eq!(inputs.num_image_tokens, 196);
        assert_eq!(inputs.pixel_values.dims(), &[784, 1536]);
        let first_patch = inputs.pixel_values.narrow(0, 0, 1)?.flatten_all()?;
        assert!(
            first_patch
                .to_vec1::<f32>()?
                .into_iter()
                .all(|value| (value - 1.0).abs() < 1e-6)
        );
        Ok(())
    }

    #[test]
    fn preprocess_rejects_processor_vision_mismatch() {
        let image = RgbImage::new(448, 448);
        let mut vision = vision_config();
        vision.patch_size = 14;
        let error = preprocess_image(
            &image,
            &processor_config(),
            &vision,
            &Device::Cpu,
            DType::F32,
        )
        .unwrap_err();
        assert!(error.to_string().contains("patch_size mismatch"));
    }

    #[test]
    fn preprocess_rejects_empty_image() {
        let error = preprocess_image(
            &RgbImage::new(0, 0),
            &processor_config(),
            &vision_config(),
            &Device::Cpu,
            DType::F32,
        )
        .unwrap_err();
        assert!(error.to_string().contains("must be non-empty"));
    }
}
