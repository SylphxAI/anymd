//! Bounded PNG encoding and cropping for rendered pages.

use std::io::{self, Cursor, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, GenericImageView, ImageEncoder, ImageFormat, ImageReader, Limits};

use super::geometry::{crop_pixels_for_rendered_page, BoundingBox, CropPixels};
use super::{RenderError, RenderedPage, DEFAULT_MAX_RENDER_OUTPUT_BYTES};

struct BoundedWriter {
    bytes: Vec<u8>,
    maximum: usize,
    exceeded: bool,
}

impl BoundedWriter {
    fn new(maximum: u64) -> Result<Self, RenderError> {
        let maximum = usize::try_from(maximum).map_err(|_| {
            RenderError::limit_exceeded("Output byte limit exceeds this platform's capacity.")
        })?;
        Ok(Self {
            bytes: Vec::new(),
            maximum,
            exceeded: false,
        })
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let remaining = self.maximum.saturating_sub(self.bytes.len());
        if buffer.len() > remaining {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "encoded PNG exceeds output byte limit",
            ));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn encode_rgba_png_bounded(
    rgba: &[u8],
    width: u32,
    height: u32,
    max_output_bytes: u64,
) -> Result<Vec<u8>, RenderError> {
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| RenderError::limit_exceeded("RGBA buffer dimensions overflow."))?;
    if u64::try_from(rgba.len()).ok() != Some(expected) {
        return Err(RenderError::encode_failed(
            "Renderer returned an RGBA buffer with an invalid length.",
        ));
    }

    let mut writer = BoundedWriter::new(max_output_bytes)?;
    let result =
        PngEncoder::new(&mut writer).write_image(rgba, width, height, ExtendedColorType::Rgba8);
    if writer.exceeded {
        return Err(RenderError::limit_exceeded(format!(
            "Encoded PNG exceeds max_output_bytes_per_page {max_output_bytes}."
        )));
    }
    result.map_err(|error| {
        RenderError::encode_failed(format!("Failed to encode deterministic RGBA PNG: {error}"))
    })?;
    Ok(writer.bytes)
}

/// Crop an encoded rendered-page PNG using already validated pixel coordinates.
pub fn crop_rendered_page_png(
    page_png: &[u8],
    expected_width: u32,
    expected_height: u32,
    crop: CropPixels,
) -> Result<Vec<u8>, RenderError> {
    crop_rendered_page_png_with_limit(
        page_png,
        expected_width,
        expected_height,
        crop,
        DEFAULT_MAX_RENDER_OUTPUT_BYTES,
    )
}

pub fn crop_rendered_page_png_with_limit(
    page_png: &[u8],
    expected_width: u32,
    expected_height: u32,
    crop: CropPixels,
    max_output_bytes: u64,
) -> Result<Vec<u8>, RenderError> {
    if max_output_bytes == 0 {
        return Err(RenderError::limit_exceeded(
            "max_output_bytes must be greater than zero.",
        ));
    }
    let raw_bytes = u64::from(expected_width)
        .checked_mul(u64::from(expected_height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| RenderError::limit_exceeded("Rendered PNG dimensions overflow."))?;
    let mut reader = ImageReader::with_format(Cursor::new(page_png), ImageFormat::Png);
    let mut decode_limits = Limits::default();
    decode_limits.max_image_width = Some(expected_width);
    decode_limits.max_image_height = Some(expected_height);
    decode_limits.max_alloc = Some(raw_bytes.saturating_mul(2).saturating_add(1_048_576));
    reader.limits(decode_limits);
    let source = catch_unwind(AssertUnwindSafe(|| reader.decode()))
        .map_err(|_| RenderError::invalid_request("PNG decoder panicked on malformed input."))?
        .map_err(|err| RenderError::invalid_request(format!("Invalid rendered page PNG: {err}")))?;
    if source.dimensions() != (expected_width, expected_height) {
        return Err(RenderError::invalid_request(format!(
            "Rendered page PNG dimensions {:?} do not match expected {expected_width}x{expected_height}.",
            source.dimensions()
        )));
    }
    let right = crop.left.checked_add(crop.width);
    let bottom = crop.top.checked_add(crop.height);
    if crop.width == 0
        || crop.height == 0
        || right.is_none_or(|value| value > expected_width)
        || bottom.is_none_or(|value| value > expected_height)
    {
        return Err(RenderError::invalid_request(
            "Crop pixels must be non-empty and contained within the rendered page.",
        ));
    }

    let cropped = source
        .crop_imm(crop.left, crop.top, crop.width, crop.height)
        .to_rgba8();
    encode_rgba_png_bounded(cropped.as_raw(), crop.width, crop.height, max_output_bytes)
}

/// Transform and crop a PDF-coordinate region from an already rendered page.
pub fn crop_rendered_page_region(
    page: &RenderedPage,
    bounding_box: BoundingBox,
    padding: f64,
    max_output_bytes: u64,
) -> Result<(CropPixels, Vec<u8>), RenderError> {
    let crop = crop_pixels_for_rendered_page(page, bounding_box, padding)?;
    let png = crop_rendered_page_png_with_limit(
        &page.png,
        page.width,
        page.height,
        crop,
        max_output_bytes,
    )?;
    Ok((crop, png))
}
