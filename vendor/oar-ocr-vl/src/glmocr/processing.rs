use super::config::{GlmOcrImageProcessorConfig, GlmOcrVisionConfig};
use crate::error::Error;
use crate::utils::{
    candle_to_ocr_processing,
    image::{image_to_chw, patchify_merge_grouped, pil_resample_to_filter_type},
};
use candle_core::{DType, Device, Tensor};
use image::{RgbImage, imageops::FilterType};

#[derive(Debug, Clone)]
pub struct GlmOcrImageInputs {
    pub pixel_values: Tensor,
    pub grid_thw: (usize, usize, usize),
    pub num_image_tokens: usize,
}

fn smart_resize_glm(
    num_frames: usize,
    height: u32,
    width: u32,
    temporal_factor: usize,
    factor: u32,
    min_pixels: u32,
    max_pixels: u32,
) -> Result<(u32, u32), Error> {
    if num_frames < temporal_factor {
        return Err(Error::InvalidInput {
            message: format!(
                "GLM-OCR smart_resize: num_frames ({num_frames}) < temporal_factor ({temporal_factor})"
            ),
        });
    }
    if factor == 0 {
        return Err(Error::InvalidInput {
            message: "GLM-OCR smart_resize: factor must be > 0".to_string(),
        });
    }

    let mut height = height as f64;
    let mut width = width as f64;
    let factor_f = factor as f64;

    if height < factor_f {
        width = (width * factor_f / height).round();
        height = factor_f;
    }
    if width < factor_f {
        height = (height * factor_f / width).round();
        width = factor_f;
    }

    let max_dim = height.max(width);
    let min_dim = height.min(width);
    if min_dim > 0.0 && (max_dim / min_dim) > 200.0 {
        return Err(Error::InvalidInput {
            message: format!(
                "GLM-OCR smart_resize: absolute aspect ratio must be <= 200, got {:.3}",
                max_dim / min_dim
            ),
        });
    }

    let mut h_bar = (height / factor_f).round() * factor_f;
    let mut w_bar = (width / factor_f).round() * factor_f;
    let t_bar = (num_frames as f64 / temporal_factor as f64).round() * temporal_factor as f64;

    let volume = t_bar * h_bar * w_bar;
    if volume > max_pixels as f64 {
        let beta = ((num_frames as f64 * height * width) / max_pixels as f64).sqrt();
        h_bar = ((height / beta) / factor_f).floor() * factor_f;
        w_bar = ((width / beta) / factor_f).floor() * factor_f;
        if h_bar < factor_f {
            h_bar = factor_f;
        }
        if w_bar < factor_f {
            w_bar = factor_f;
        }
    } else if volume < min_pixels as f64 {
        let beta = (min_pixels as f64 / (num_frames as f64 * height * width)).sqrt();
        h_bar = ((height * beta) / factor_f).ceil() * factor_f;
        w_bar = ((width * beta) / factor_f).ceil() * factor_f;
    }

    Ok((h_bar as u32, w_bar as u32))
}

pub fn preprocess_image(
    image: &RgbImage,
    cfg: &GlmOcrImageProcessorConfig,
    vision_cfg: &GlmOcrVisionConfig,
    device: &Device,
    dtype: DType,
) -> Result<GlmOcrImageInputs, Error> {
    cfg.validate()?;
    if cfg.patch_size != vision_cfg.patch_size {
        return Err(Error::Config {
            message: format!(
                "GLM-OCR patch_size mismatch: preprocessor {} != vision_config {}",
                cfg.patch_size, vision_cfg.patch_size
            ),
        });
    }
    if cfg.temporal_patch_size != vision_cfg.temporal_patch_size {
        return Err(Error::Config {
            message: format!(
                "GLM-OCR temporal_patch_size mismatch: preprocessor {} != vision_config {}",
                cfg.temporal_patch_size, vision_cfg.temporal_patch_size
            ),
        });
    }
    if cfg.merge_size != vision_cfg.spatial_merge_size {
        return Err(Error::Config {
            message: format!(
                "GLM-OCR merge_size mismatch: preprocessor {} != vision_config {}",
                cfg.merge_size, vision_cfg.spatial_merge_size
            ),
        });
    }

    let resize_filter = cfg
        .resample
        .and_then(pil_resample_to_filter_type)
        .unwrap_or(FilterType::CatmullRom);

    let (h, w) = (image.height(), image.width());
    let factor = (cfg.patch_size * cfg.merge_size) as u32;
    let (rh, rw) = if cfg.do_resize {
        smart_resize_glm(
            cfg.temporal_patch_size,
            h,
            w,
            cfg.temporal_patch_size,
            factor,
            cfg.size.shortest_edge,
            cfg.size.longest_edge,
        )?
    } else {
        (h, w)
    };

    let resized = if rh != h || rw != w {
        image::imageops::resize(image, rw, rh, resize_filter)
    } else {
        image.clone()
    };

    if rh % factor != 0 || rw % factor != 0 {
        return Err(Error::Config {
            message: format!(
                "GLM-OCR preprocess produced non-divisible dims: {rh}x{rw} not divisible by factor={factor}"
            ),
        });
    }

    let mean = if cfg.do_normalize {
        cfg.image_mean.clone()
    } else {
        vec![0.0, 0.0, 0.0]
    };
    let std = if cfg.do_normalize {
        cfg.image_std.clone()
    } else {
        vec![1.0, 1.0, 1.0]
    };

    let rescale_factor = if cfg.do_rescale {
        Some(cfg.rescale_factor)
    } else {
        None
    };

    let chw = image_to_chw(&resized, &mean, &std, rescale_factor);

    let channel = 3usize;
    let height = rh as usize;
    let width = rw as usize;
    let patch_size = cfg.patch_size;
    let merge_size = cfg.merge_size;
    let temporal_patch = cfg.temporal_patch_size;

    // For static images the single CHW frame is repeated `temporal_patch` times
    // to fill the temporal dimension expected by the vision encoder.
    let frames: Vec<&[f32]> = std::iter::repeat_n(chw.as_slice(), temporal_patch).collect();

    let grid_t = frames.len() / temporal_patch;
    let grid_h = height / patch_size;
    let grid_w = width / patch_size;

    if !grid_h.is_multiple_of(merge_size) || !grid_w.is_multiple_of(merge_size) {
        return Err(Error::Config {
            message: format!(
                "GLM-OCR preprocess produced non-divisible grid: grid_h={grid_h}, grid_w={grid_w}, merge_size={merge_size}"
            ),
        });
    }

    let patch_dim = channel * temporal_patch * patch_size * patch_size;
    let num_patches = grid_t * grid_h * grid_w;

    let flat = patchify_merge_grouped(
        &frames,
        channel,
        height,
        width,
        grid_t,
        grid_h,
        grid_w,
        patch_size,
        merge_size,
        temporal_patch,
    );

    let num_image_tokens = num_patches / (merge_size * merge_size);
    let pixel_values = Tensor::from_vec(flat, (num_patches, patch_dim), device)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: failed to create pixel_values tensor",
                e,
            )
        })?
        .to_dtype(dtype)
        .map_err(|e| {
            candle_to_ocr_processing(
                crate::error::ProcessingStage::TensorOperation,
                "GLM-OCR: failed to cast pixel_values dtype",
                e,
            )
        })?;

    Ok(GlmOcrImageInputs {
        pixel_values,
        grid_thw: (grid_t, grid_h, grid_w),
        num_image_tokens,
    })
}
