//! PP-DocLayout image preprocessing.
//!
//! `preprocessor_config.json` asks for a bicubic resize to 800x800, a `1/255`
//! rescale and an identity normalization (mean 0, std 1). The filter matches
//! the one `oar-ocr-core`'s ONNX PP-DocLayout path uses, so both backends see
//! the same pixels.

use super::err::proc_err;
use crate::error::Error;
use candle_core::{DType, Device, Tensor};
use image::RgbImage;
use image::imageops::FilterType;

/// Input size both checkpoints are trained and exported at.
pub(super) const INPUT_SIZE: u32 = 800;

/// Model input plus the source dimensions needed to map boxes back.
pub(super) struct PreprocessedImage {
    /// `(1, 3, 800, 800)` float tensor.
    pub pixel_values: Tensor,
    pub original_width: u32,
    pub original_height: u32,
}

/// Resizes to the fixed 800x800 input and scales pixels into `[0, 1]`.
pub(super) fn preprocess(
    image: &RgbImage,
    device: &Device,
    dtype: DType,
) -> Result<PreprocessedImage, Error> {
    let (original_width, original_height) = (image.width(), image.height());
    if original_width == 0 || original_height == 0 {
        return Err(Error::Config {
            message: "PP-DocLayout: input image has zero width or height".to_string(),
        });
    }

    let resized = image::imageops::resize(image, INPUT_SIZE, INPUT_SIZE, FilterType::CatmullRom);

    let pixels = INPUT_SIZE as usize * INPUT_SIZE as usize;
    let mut data = vec![0f32; 3 * pixels];
    for (index, pixel) in resized.pixels().enumerate() {
        data[index] = pixel[0] as f32 / 255.0;
        data[pixels + index] = pixel[1] as f32 / 255.0;
        data[2 * pixels + index] = pixel[2] as f32 / 255.0;
    }

    let pixel_values = Tensor::from_vec(
        data,
        (1, 3, INPUT_SIZE as usize, INPUT_SIZE as usize),
        device,
    )
    .and_then(|t| t.to_dtype(dtype))
    .map_err(|e| proc_err("build PP-DocLayout input tensor", e))?;

    Ok(PreprocessedImage {
        pixel_values,
        original_width,
        original_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_a_normalized_fixed_size_tensor() {
        let image = RgbImage::from_pixel(37, 11, image::Rgb([255, 0, 128]));
        let prepared = preprocess(&image, &Device::Cpu, DType::F32).unwrap();
        assert_eq!(
            prepared.pixel_values.dims(),
            &[1, 3, INPUT_SIZE as usize, INPUT_SIZE as usize]
        );
        assert_eq!(prepared.original_width, 37);
        assert_eq!(prepared.original_height, 11);

        let values = prepared
            .pixel_values
            .flatten_all()
            .unwrap()
            .to_vec1::<f32>()
            .unwrap();
        assert!(values.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!((values[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_empty_images() {
        let image = RgbImage::new(0, 0);
        assert!(preprocess(&image, &Device::Cpu, DType::F32).is_err());
    }
}
