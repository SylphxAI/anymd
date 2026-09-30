use crate::error::Error;
use image::RgbImage;
use image::imageops::FilterType;
use rayon::prelude::*;

/// Convert an RGB image to a CHW tensor buffer with normalization and parallel processing.
///
/// Output layout: [R0...Rn, G0...Gn, B0...Bn]
pub fn image_to_chw(
    image: &RgbImage,
    mean: &[f32],
    std: &[f32],
    rescale_factor: Option<f32>,
) -> Vec<f32> {
    // RGB layout of the source buffer: 3 interleaved channels per pixel.
    const RGB_CHANNELS: usize = 3;
    const R: usize = 0;
    const G: usize = 1;
    const B: usize = 2;

    let width = image.width() as usize;
    let height = image.height() as usize;
    let num_pixels = width * height;
    let scale = rescale_factor.unwrap_or(1.0);

    let mut output = vec![0.0f32; num_pixels * RGB_CHANNELS];
    let raw_pixels = image.as_raw();

    // Split the output into the three contiguous channel planes. Each plane is
    // a disjoint `&mut [f32]`, so writing them concurrently is safe — no raw
    // pointers or `UnsafeSlice` needed.
    let (r_plane, rest) = output.split_at_mut(num_pixels);
    let (g_plane, b_plane) = rest.split_at_mut(num_pixels);

    // Parallel iteration over pixels: each iteration writes one element in each
    // plane at the same index, and the three planes are non-overlapping.
    // `par_chunks_exact` walks `raw_pixels` in `RGB_CHANNELS`-sized strides so
    // the compiler can elide bounds checks on the source indexing.
    raw_pixels
        .par_chunks_exact(RGB_CHANNELS)
        .zip(r_plane.par_iter_mut())
        .zip(g_plane.par_iter_mut())
        .zip(b_plane.par_iter_mut())
        .for_each(|(((chunk, r_out), g_out), b_out)| {
            let r = chunk[R] as f32 * scale;
            let g = chunk[G] as f32 * scale;
            let b = chunk[B] as f32 * scale;

            *r_out = (r - mean[0]) / std[0];
            *g_out = (g - mean[1]) / std[1];
            *b_out = (b - mean[2]) / std[2];
        });

    output
}

/// Rearrange CHW frame buffers into Qwen2-VL style merge-grouped flat patches.
///
/// This is the shared "patchify" used by GLM-OCR and MinerU2.5 preprocessing:
/// it flattens per-frame CHW pixel buffers into the `pixel_values` layout
/// `(num_patches, patch_dim)` expected by the vision encoder, where
/// `patch_dim = channel * temporal_patch * patch_size * patch_size`.
///
/// Patch ordering follows the spatial-merge convention: iterate
/// `(temporal, h-block, w-block, h-in-block, w-in-block)`, and each patch is
/// laid out as `[channel][temporal][ph][pw]`.
///
/// # Arguments
///
/// * `frames` - `grid_t * temporal_patch` CHW buffers, each of length
///   `channel * height * width`. Frame `t * temporal_patch + tp` is the `tp`-th
///   temporal sample of grid-time `t`.
/// * `channel`, `height`, `width` - dimensions of each CHW frame buffer.
/// * `grid_t`, `grid_h`, `grid_w` - patch grid dimensions. `grid_h` and
///   `grid_w` must be multiples of `merge_size` (callers validate this).
/// * `patch_size`, `merge_size`, `temporal_patch` - patch/merge configuration.
///
/// # Returns
///
/// A flat `Vec<f32>` of length `grid_t * grid_h * grid_w * patch_dim`, ready to
/// be wrapped as a `(num_patches, patch_dim)` tensor.
#[allow(clippy::too_many_arguments)]
pub fn patchify_merge_grouped(
    frames: &[&[f32]],
    channel: usize,
    height: usize,
    width: usize,
    grid_t: usize,
    grid_h: usize,
    grid_w: usize,
    patch_size: usize,
    merge_size: usize,
    temporal_patch: usize,
) -> Vec<f32> {
    debug_assert_eq!(
        frames.len(),
        grid_t * temporal_patch,
        "patchify_merge_grouped: expected {} frames (grid_t * temporal_patch), got {}",
        grid_t * temporal_patch,
        frames.len()
    );
    debug_assert!(
        frames.iter().all(|f| f.len() == channel * height * width),
        "patchify_merge_grouped: every frame must have length channel * height * width"
    );

    let patch_dim = channel * temporal_patch * patch_size * patch_size;
    let num_patches = grid_t * grid_h * grid_w;
    let mut flat = Vec::with_capacity(num_patches * patch_dim);

    let channel_stride = height * width;
    let grid_h_blocks = grid_h / merge_size;
    let grid_w_blocks = grid_w / merge_size;

    for t in 0..grid_t {
        for hb in 0..grid_h_blocks {
            for wb in 0..grid_w_blocks {
                for hm in 0..merge_size {
                    for wm in 0..merge_size {
                        let patch_row = hb * merge_size + hm;
                        let patch_col = wb * merge_size + wm;
                        for c in 0..channel {
                            for tp in 0..temporal_patch {
                                let frame = frames[t * temporal_patch + tp];
                                let base = c * channel_stride;
                                for ph in 0..patch_size {
                                    let h_idx = patch_row * patch_size + ph;
                                    let row_base = base + h_idx * width;
                                    for pw in 0..patch_size {
                                        let w_idx = patch_col * patch_size + pw;
                                        flat.push(frame[row_base + w_idx]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    flat
}

/// Convert PIL/Transformers integer resampling constants to `image::FilterType`.
///
/// Mappings:
/// 0 => Nearest
/// 1 => Lanczos3
/// 2 => Triangle (Bilinear)
/// 3 => CatmullRom (Bicubic)
/// 4 => Box
/// 5 => Hamming
pub fn pil_resample_to_filter_type(resample: u32) -> Option<FilterType> {
    match resample {
        0 => Some(FilterType::Nearest),
        1 => Some(FilterType::Lanczos3),
        2 => Some(FilterType::Triangle),
        3 => Some(FilterType::CatmullRom),
        4 => Some(FilterType::Triangle), // Box map to Triangle as approx or specific if available
        5 => Some(FilterType::CatmullRom), // Hamming map to CatmullRom as approx
        _ => None,
    }
}

/// Round up a value to the next multiple of a factor.
pub fn round_up_to_multiple(value: u32, multiple: u32) -> u32 {
    if multiple == 0 {
        return value;
    }
    value.div_ceil(multiple) * multiple
}

/// Smart resize calculating new dimensions based on pixel constraints and factor alignment.
///
/// Used by PaddleOCR-VL and HunyuanOCR.
pub fn smart_resize(
    height: u32,
    width: u32,
    factor: u32,
    min_pixels: u32,
    max_pixels: u32,
) -> Result<(u32, u32), Error> {
    if factor == 0 {
        return Err(Error::InvalidInput {
            message: "smart_resize: factor must be > 0".to_string(),
        });
    }

    let height = height as f64;
    let width = width as f64;
    let factor_f = factor as f64;

    let max_dim = height.max(width);
    let min_dim = height.min(width);
    if min_dim > 0.0 && (max_dim / min_dim) > 200.0 {
        return Err(Error::InvalidInput {
            message: format!(
                "smart_resize: absolute aspect ratio must be <= 200, got {:.3}",
                max_dim / min_dim
            ),
        });
    }

    let mut h_bar = (height / factor_f).round() * factor_f;
    let mut w_bar = (width / factor_f).round() * factor_f;

    let area = h_bar * w_bar;
    if area > max_pixels as f64 {
        let beta = ((height * width) / max_pixels as f64).sqrt();
        h_bar = ((height / beta) / factor_f).floor() * factor_f;
        w_bar = ((width / beta) / factor_f).floor() * factor_f;
        if h_bar < factor_f {
            h_bar = factor_f;
        }
        if w_bar < factor_f {
            w_bar = factor_f;
        }
        // After scaling down, ensure we don't violate min_pixels due to quantization
        if (h_bar * w_bar) < min_pixels as f64 {
            return Err(Error::InvalidInput {
                message: format!(
                    "smart_resize: cannot satisfy both min_pixels={} and max_pixels={} constraints after quantization",
                    min_pixels, max_pixels
                ),
            });
        }
    } else if area < min_pixels as f64 {
        let beta = (min_pixels as f64 / (height * width)).sqrt();
        h_bar = ((height * beta) / factor_f).ceil() * factor_f;
        w_bar = ((width * beta) / factor_f).ceil() * factor_f;
        // Ensure minimum dimensions are at least factor_f
        if h_bar < factor_f {
            h_bar = factor_f;
        }
        if w_bar < factor_f {
            w_bar = factor_f;
        }
        // After scaling up, ensure we don't violate max_pixels due to quantization
        if (h_bar * w_bar) > max_pixels as f64 {
            return Err(Error::InvalidInput {
                message: format!(
                    "smart_resize: cannot satisfy both min_pixels={} and max_pixels={} constraints after quantization",
                    min_pixels, max_pixels
                ),
            });
        }
    }

    Ok((h_bar as u32, w_bar as u32))
}

/// Clamp dimensions to a maximum image size while maintaining aspect ratio and factor divisibility.
///
/// Used by HunyuanOCR.
pub fn clamp_to_max_image_size(
    height: u32,
    width: u32,
    factor: u32,
    max_image_size: usize,
) -> Result<(u32, u32), Error> {
    if factor == 0 {
        return Err(Error::InvalidInput {
            message: "clamp_to_max_image_size: factor must be > 0".to_string(),
        });
    }
    let max_image_size = u32::try_from(max_image_size).map_err(|_| Error::Config {
        message: format!("max_image_size too large: {max_image_size}"),
    })?;
    if max_image_size == 0 {
        return Err(Error::Config {
            message: "max_image_size must be > 0".to_string(),
        });
    }
    if max_image_size < factor {
        return Err(Error::Config {
            message: format!("max_image_size {max_image_size} must be >= factor={factor}"),
        });
    }

    let max_dim = height.max(width);
    if max_dim <= max_image_size {
        return Ok((height, width));
    }

    let scale = max_image_size as f64 / max_dim as f64;
    let factor_f = factor as f64;

    let mut h = (((height as f64) * scale) / factor_f).floor() * factor_f;
    let mut w = (((width as f64) * scale) / factor_f).floor() * factor_f;

    // Ensure non-zero dims and factor divisibility.
    if h < factor_f {
        h = factor_f;
    }
    if w < factor_f {
        w = factor_f;
    }

    let h = h as u32;
    let w = w as u32;

    Ok((h, w))
}

/// Resize an image for MinerU2.5 inference.
///
/// Handles two cases:
/// 1. If the aspect ratio exceeds `max_aspect_ratio`, the image is padded with
///    white to bring it within bounds.
/// 2. If the minimum edge is below `min_edge`, the image is scaled up.
pub fn resize_for_mineru(image: &RgbImage, min_edge: u32, max_aspect_ratio: f32) -> RgbImage {
    use image::{Rgb, imageops};
    use std::borrow::Cow;

    let (mut w, mut h) = image.dimensions();
    let mut out = Cow::Borrowed(image);

    // Handle extreme aspect ratios by padding
    let ratio = (w.max(h) as f32) / (w.min(h).max(1) as f32);
    if ratio > max_aspect_ratio {
        let (new_w, new_h) = if w > h {
            (w, (w as f32 / max_aspect_ratio).ceil() as u32)
        } else {
            ((h as f32 / max_aspect_ratio).ceil() as u32, h)
        };
        let mut canvas = RgbImage::from_pixel(new_w, new_h, Rgb([255, 255, 255]));
        let x = ((new_w - w) / 2) as i64;
        let y = ((new_h - h) / 2) as i64;
        imageops::overlay(&mut canvas, &*out, x, y);
        out = Cow::Owned(canvas);
        (w, h) = out.dimensions();
    }

    // Scale up if minimum edge is too small
    let min_dim = w.min(h);
    if min_dim < min_edge {
        let scale = min_edge as f32 / min_dim as f32;
        let new_w = (w as f32 * scale).ceil() as u32;
        let new_h = (h as f32 * scale).ceil() as u32;
        out = Cow::Owned(imageops::resize(
            &*out,
            new_w,
            new_h,
            imageops::FilterType::CatmullRom,
        ));
    }

    out.into_owned()
}

/// Loads an image from disk and converts it to RGB.
///
/// The format comes from the file's contents, not its name, so extensionless
/// and mislabelled files load. `image::open` would pick the decoder from the
/// path suffix and reject both.
pub fn load_image(path: impl AsRef<std::path::Path>) -> Result<RgbImage, crate::Error> {
    let file = std::io::BufReader::new(std::fs::File::open(path.as_ref())?);
    let decoded = image::ImageReader::new(file)
        .with_guessed_format()?
        .decode()?;
    Ok(decoded.to_rgb8())
}

/// Decodes an image from memory and converts it to RGB.
pub fn load_image_from_memory(bytes: &[u8]) -> Result<RgbImage, crate::Error> {
    Ok(image::load_from_memory(bytes)?.to_rgb8())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Callers pass paths straight from a CLI, where an extensionless or
    /// mislabelled file is ordinary. The format must come from the bytes.
    #[test]
    fn loads_images_whose_name_does_not_match_their_format() {
        let dir = std::env::temp_dir().join("oar_vl_load_image_by_content");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let mut png = Vec::new();
        RgbImage::from_pixel(3, 2, ::image::Rgb([1, 2, 3]))
            .write_with_encoder(::image::codecs::png::PngEncoder::new(&mut png))
            .expect("encode png");

        for name in ["no_extension", "actually_a_png.jpg"] {
            let path = dir.join(name);
            std::fs::write(&path, &png).expect("write fixture");
            let loaded = load_image(&path).expect("load by content");
            assert_eq!(loaded.dimensions(), (3, 2));
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn test_patchify_merge_grouped_ordering() {
        // channel=2, height=width=4, patch_size=2, merge_size=2 => one 2x2
        // merge block of 2x2-pixel patches, temporal=1, grid_t=1.
        let channel = 2usize;
        let (height, width) = (4usize, 4usize);
        let (patch_size, merge_size, temporal) = (2usize, 2usize, 1usize);
        let (grid_t, grid_h, grid_w) = (1usize, 2usize, 2usize);

        // frame[c*16 + y*4 + x] = c*100 + y*10 + x
        let mut frame = vec![0.0f32; channel * height * width];
        for c in 0..channel {
            for y in 0..height {
                for x in 0..width {
                    frame[c * height * width + y * width + x] = (c * 100 + y * 10 + x) as f32;
                }
            }
        }
        let frames: Vec<&[f32]> = vec![frame.as_slice()];

        let flat = patchify_merge_grouped(
            &frames, channel, height, width, grid_t, grid_h, grid_w, patch_size, merge_size,
            temporal,
        );

        // Expected order: (hm, wm) over the merge block, then channel, then the
        // 2x2 pixels of each patch laid out row-major.
        let expected: Vec<f32> = vec![
            0., 1., 10., 11., 100., 101., 110., 111., // hm0,wm0: c0 then c1
            2., 3., 12., 13., 102., 103., 112., 113., // hm0,wm1
            20., 21., 30., 31., 120., 121., 130., 131., // hm1,wm0
            22., 23., 32., 33., 122., 123., 132., 133., // hm1,wm1
        ];
        assert_eq!(flat, expected);
        // num_patches * patch_dim = (1*2*2) * (2*1*2*2) = 4 * 8 = 32
        assert_eq!(flat.len(), 32);
    }

    #[test]
    fn test_patchify_merge_grouped_merge1_is_raster() {
        // merge_size=1 => patches are emitted in raster (gh, gw) order, each
        // patch being a contiguous channel-major block.
        let channel = 1usize;
        let (height, width) = (2usize, 4usize);
        let (patch_size, merge_size, temporal) = (2usize, 1usize, 1usize);
        let (grid_t, grid_h, grid_w) = (1usize, 1usize, 2usize);

        let mut frame = vec![0.0f32; channel * height * width];
        for y in 0..height {
            for x in 0..width {
                frame[y * width + x] = (y * width + x) as f32;
            }
        }
        let frames: Vec<&[f32]> = vec![frame.as_slice()];

        let flat = patchify_merge_grouped(
            &frames, channel, height, width, grid_t, grid_h, grid_w, patch_size, merge_size,
            temporal,
        );

        // Patch (0,0): x in {0,1}; patch (0,1): x in {2,3}
        let expected: Vec<f32> = vec![
            0., 1., 4., 5., // first patch
            2., 3., 6., 7., // second patch
        ];
        assert_eq!(flat, expected);
    }

    #[test]
    fn test_smart_resize_factor_divisibility() -> Result<(), Error> {
        let (h, w) = smart_resize(100, 200, 28, 147_384, 2_822_400)?;
        assert_eq!(h % 28, 0);
        assert_eq!(w % 28, 0);
        Ok(())
    }

    #[test]
    fn test_clamp_to_max_image_size_keeps_factor_divisible() -> Result<(), Error> {
        let factor = 32;
        let (h, w) = clamp_to_max_image_size(3008, 512, factor, 2048)?;
        assert!(h <= 2048);
        assert!(w <= 2048);
        assert_eq!(h % factor, 0);
        assert_eq!(w % factor, 0);
        Ok(())
    }
}
