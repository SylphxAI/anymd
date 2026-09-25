//! Bounded, pure-Rust PDF page rendering and region cropping primitives.

use std::io::{self, Cursor, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

use hayro::hayro_interpret::hayro_syntax::page::Rotation;
use hayro::hayro_interpret::hayro_syntax::{LoadPdfError, Pdf};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, RenderCache, RenderSettings};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, GenericImageView, ImageEncoder, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};

pub const DEFAULT_RENDER_SCALE: f32 = 2.0;
pub const DEFAULT_MAX_RENDER_PIXELS: u64 = 16_000_000;
pub const DEFAULT_MAX_RENDER_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_MAX_RENDER_PAGES: usize = 20;
pub const RENDER_ENGINE: &str = "hayro";
pub const RENDERER_NAME: &str = "hayro/vello_cpu";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderErrorCode {
    InvalidRequest,
    InvalidPdf,
    EncryptedPdf,
    LimitExceeded,
    EncodeFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderError {
    pub code: RenderErrorCode,
    pub message: String,
}

impl RenderError {
    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::InvalidRequest,
            message: message.into(),
        }
    }

    fn invalid_pdf(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::InvalidPdf,
            message: message.into(),
        }
    }

    fn encrypted_pdf(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::EncryptedPdf,
            message: message.into(),
        }
    }

    fn limit_exceeded(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::LimitExceeded,
            message: message.into(),
        }
    }

    fn encode_failed(message: impl Into<String>) -> Self {
        Self {
            code: RenderErrorCode::EncodeFailed,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CropPixels {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderProvenance {
    pub engine: &'static str,
    pub renderer: &'static str,
    pub source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderedPage {
    pub page: usize,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub pixel_count: u64,
    pub rotation: i32,
    pub format: &'static str,
    pub mime_type: &'static str,
    pub provenance: RenderProvenance,
    #[serde(skip)]
    pub png: Vec<u8>,
    #[serde(skip)]
    pdf_to_pixel: [f64; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderLimits {
    pub max_pages: usize,
    pub max_pixels_per_page: u64,
    pub max_output_bytes_per_page: u64,
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            max_pages: DEFAULT_MAX_RENDER_PAGES,
            max_pixels_per_page: DEFAULT_MAX_RENDER_PIXELS,
            max_output_bytes_per_page: DEFAULT_MAX_RENDER_OUTPUT_BYTES,
        }
    }
}

/// Parsed, reusable PDF rendering document. The PDF bytes are owned by Hayro,
/// so selected pages can be rendered without reparsing the document per page.
pub struct RenderDocument {
    pdf: Pdf,
}

impl RenderDocument {
    pub fn new(pdf_bytes: Vec<u8>) -> Result<Self, RenderError> {
        let parsed = catch_unwind(AssertUnwindSafe(|| Pdf::new(pdf_bytes))).map_err(|_| {
            RenderError::invalid_pdf("PDF parser panicked while loading the document.")
        })?;
        parsed.map(|pdf| Self { pdf }).map_err(map_load_error)
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Compute the pixel work for a page before invoking the renderer.
    pub fn planned_render_pixel_count(
        &self,
        page_number: usize,
        scale: f32,
    ) -> Result<u64, RenderError> {
        catch_unwind(AssertUnwindSafe(|| {
            if page_number == 0 {
                return Err(RenderError::invalid_request("page must be 1 or greater."));
            }
            if !scale.is_finite() || scale <= 0.0 {
                return Err(RenderError::invalid_request(
                    "scale must be a finite positive number.",
                ));
            }
            let page = self.pdf.pages().get(page_number - 1).ok_or_else(|| {
                RenderError::invalid_request(format!(
                    "Requested page {page_number} exceeds document page count {}.",
                    self.page_count()
                ))
            })?;
            planned_dimensions(page, scale, page_number).map(|(_, _, pixels)| pixels)
        }))
        .map_err(|_| RenderError::invalid_pdf("PDF parser panicked while planning a render."))?
    }

    pub fn render_page(
        &self,
        page_number: usize,
        scale: f32,
        max_pixels: u64,
        max_output_bytes: u64,
    ) -> Result<RenderedPage, RenderError> {
        catch_unwind(AssertUnwindSafe(|| {
            self.render_page_impl(page_number, scale, max_pixels, max_output_bytes)
        }))
        .map_err(|_| RenderError::invalid_pdf("PDF renderer panicked on malformed content."))?
    }

    pub fn render_selected_pages(
        &self,
        page_numbers: &[usize],
        scale: f32,
        limits: RenderLimits,
    ) -> Result<Vec<RenderedPage>, RenderError> {
        validate_limits(limits)?;
        if page_numbers.is_empty() {
            return Err(RenderError::invalid_request(
                "At least one page must be selected for rendering.",
            ));
        }
        if page_numbers.len() > limits.max_pages {
            return Err(RenderError::limit_exceeded(format!(
                "Requested {} pages, exceeding max_pages {}.",
                page_numbers.len(),
                limits.max_pages
            )));
        }
        page_numbers
            .iter()
            .map(|page| {
                self.render_page(
                    *page,
                    scale,
                    limits.max_pixels_per_page,
                    limits.max_output_bytes_per_page,
                )
            })
            .collect()
    }

    fn render_page_impl(
        &self,
        page_number: usize,
        scale: f32,
        max_pixels: u64,
        max_output_bytes: u64,
    ) -> Result<RenderedPage, RenderError> {
        validate_render_request(page_number, scale, max_pixels, max_output_bytes)?;
        let page = self.pdf.pages().get(page_number - 1).ok_or_else(|| {
            RenderError::invalid_request(format!(
                "Requested page {page_number} exceeds document page count {}.",
                self.page_count()
            ))
        })?;
        let (width, height, pixel_count) = planned_dimensions(page, scale, page_number)?;
        if pixel_count > max_pixels {
            return Err(RenderError::limit_exceeded(format!(
                "Page {page_number} would render {pixel_count} pixels at scale {scale}, exceeding max_pixels_per_page {max_pixels}. Lower scale or raise max_pixels_per_page."
            )));
        }

        let viewport_width = u16::try_from(width).map_err(|_| {
            RenderError::limit_exceeded(format!(
                "Page {page_number} render width {width} exceeds renderer limit {}.",
                u16::MAX
            ))
        })?;
        let viewport_height = u16::try_from(height).map_err(|_| {
            RenderError::limit_exceeded(format!(
                "Page {page_number} render height {height} exceeds renderer limit {}.",
                u16::MAX
            ))
        })?;

        let pixmap = render(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &RenderSettings {
                x_scale: scale,
                y_scale: scale,
                width: Some(viewport_width),
                height: Some(viewport_height),
                bg_color: WHITE,
            },
        );
        let rgba = pixmap.data_as_u8_slice();
        if rgba.chunks_exact(4).any(|pixel| pixel[3] != 255) {
            return Err(RenderError::encode_failed(
                "White-background render unexpectedly contained transparent pixels.",
            ));
        }
        let png = encode_rgba_png_bounded(rgba, width, height, max_output_bytes)?;
        let mut pdf_to_pixel = page.initial_transform(true).as_coeffs();
        for coefficient in &mut pdf_to_pixel {
            *coefficient *= f64::from(scale);
        }

        Ok(RenderedPage {
            page: page_number,
            width,
            height,
            scale,
            pixel_count,
            rotation: rotation_degrees(page.rotation()),
            format: "png",
            mime_type: "image/png",
            provenance: RenderProvenance {
                engine: RENDER_ENGINE,
                renderer: RENDERER_NAME,
                source: "page-render",
            },
            png,
            pdf_to_pixel,
        })
    }
}

fn planned_dimensions(
    page: &hayro::hayro_interpret::hayro_syntax::page::Page<'_>,
    scale: f32,
    page_number: usize,
) -> Result<(u32, u32, u64), RenderError> {
    let (page_width, page_height) = page.render_dimensions();
    let width = checked_render_dimension(page_width, scale, "width", page_number)?;
    let height = checked_render_dimension(page_height, scale, "height", page_number)?;
    let pixel_count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| RenderError::limit_exceeded("Rendered page dimensions overflow."))?;
    Ok((width, height, pixel_count))
}

fn map_load_error(error: LoadPdfError) -> RenderError {
    match error {
        LoadPdfError::Decryption(_) => RenderError::encrypted_pdf(
            "Encrypted PDF cannot be rendered without a supported password.",
        ),
        LoadPdfError::Invalid => RenderError::invalid_pdf("Unable to parse PDF for rendering."),
    }
}

/// Render one 1-indexed PDF page into a white-background PNG.
///
/// The pixel budget is checked from page geometry before allocating the output.
pub fn render_pdf_page(
    pdf_bytes: Vec<u8>,
    page_number: usize,
    scale: f32,
    max_pixels: u64,
) -> Result<RenderedPage, RenderError> {
    RenderDocument::new(pdf_bytes)?.render_page(
        page_number,
        scale,
        max_pixels,
        DEFAULT_MAX_RENDER_OUTPUT_BYTES,
    )
}

pub fn pdf_page_count(pdf_bytes: Vec<u8>) -> Result<usize, RenderError> {
    RenderDocument::new(pdf_bytes).map(|document| document.page_count())
}

fn validate_render_request(
    page_number: usize,
    scale: f32,
    max_pixels: u64,
    max_output_bytes: u64,
) -> Result<(), RenderError> {
    if page_number == 0 {
        return Err(RenderError::invalid_request("page must be 1 or greater."));
    }
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RenderError::invalid_request(
            "scale must be a finite positive number.",
        ));
    }
    if max_pixels == 0 {
        return Err(RenderError::limit_exceeded(
            "max_pixels_per_page must be greater than zero.",
        ));
    }
    if max_output_bytes == 0 {
        return Err(RenderError::limit_exceeded(
            "max_output_bytes_per_page must be greater than zero.",
        ));
    }
    Ok(())
}

fn validate_limits(limits: RenderLimits) -> Result<(), RenderError> {
    if limits.max_pages == 0 {
        return Err(RenderError::limit_exceeded(
            "max_pages must be greater than zero.",
        ));
    }
    validate_render_request(
        1,
        DEFAULT_RENDER_SCALE,
        limits.max_pixels_per_page,
        limits.max_output_bytes_per_page,
    )
}

fn rotation_degrees(rotation: Rotation) -> i32 {
    match rotation {
        Rotation::None => 0,
        Rotation::Horizontal => 90,
        Rotation::Flipped => 180,
        Rotation::FlippedHorizontal => 270,
    }
}

fn checked_render_dimension(
    page_points: f32,
    scale: f32,
    axis: &str,
    page_number: usize,
) -> Result<u32, RenderError> {
    let scaled = f64::from(page_points) * f64::from(scale);
    if !scaled.is_finite() || scaled <= 0.0 || scaled > f64::from(u32::MAX) {
        return Err(RenderError::invalid_request(format!(
            "Page {page_number} has invalid render {axis}."
        )));
    }
    Ok(scaled.ceil() as u32)
}

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

fn encode_rgba_png_bounded(
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

/// Convert a PDF bottom-left-origin bounding box into a clamped top-left pixel crop.
///
/// This compatibility entrypoint assumes a zero-origin page box. Use
/// [`crop_pixels_for_rendered_page`] when a parsed page is available; it carries
/// the exact CropBox offset and Hayro render transform.
pub fn crop_pixels_for_bounding_box(
    page_width: u32,
    page_height: u32,
    scale: f32,
    page_rotation: i32,
    bounding_box: BoundingBox,
    padding: f64,
) -> Result<CropPixels, RenderError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RenderError::invalid_request(
            "scale must be a finite positive number.",
        ));
    }
    if !padding.is_finite() || padding < 0.0 {
        return Err(RenderError::invalid_request(
            "padding must be a finite non-negative number.",
        ));
    }
    let scale = f64::from(scale);
    let transform = match page_rotation.rem_euclid(360) {
        0 => [scale, 0.0, 0.0, -scale, 0.0, f64::from(page_height)],
        90 => [0.0, scale, scale, 0.0, 0.0, 0.0],
        180 => [-scale, 0.0, 0.0, scale, f64::from(page_width), 0.0],
        270 => [
            0.0,
            -scale,
            -scale,
            0.0,
            f64::from(page_width),
            f64::from(page_height),
        ],
        other => {
            return Err(RenderError::invalid_request(format!(
                "Unsupported page rotation {other}; expected a multiple of 90 degrees."
            )))
        }
    };
    crop_pixels_for_transform(page_width, page_height, transform, bounding_box, padding)
}

/// Convert PDF coordinates using the exact transform used to render this page.
pub fn crop_pixels_for_rendered_page(
    page: &RenderedPage,
    bounding_box: BoundingBox,
    padding: f64,
) -> Result<CropPixels, RenderError> {
    crop_pixels_for_transform(
        page.width,
        page.height,
        page.pdf_to_pixel,
        bounding_box,
        padding,
    )
}

fn crop_pixels_for_transform(
    page_width: u32,
    page_height: u32,
    transform: [f64; 6],
    bounding_box: BoundingBox,
    padding: f64,
) -> Result<CropPixels, RenderError> {
    if !padding.is_finite() || padding < 0.0 {
        return Err(RenderError::invalid_request(
            "padding must be a finite non-negative number.",
        ));
    }
    if !valid_bounding_box(bounding_box) {
        return Err(RenderError::invalid_request(
            "Region bounding_box must have right > left and top > bottom.",
        ));
    }
    let expanded = BoundingBox {
        left: bounding_box.left - padding,
        bottom: bounding_box.bottom - padding,
        right: bounding_box.right + padding,
        top: bounding_box.top + padding,
    };
    if ![expanded.left, expanded.bottom, expanded.right, expanded.top]
        .iter()
        .all(|value| value.is_finite())
    {
        return Err(RenderError::invalid_request(
            "Region bounding_box plus padding must remain finite.",
        ));
    }

    let corners = [
        (expanded.left, expanded.bottom),
        (expanded.left, expanded.top),
        (expanded.right, expanded.bottom),
        (expanded.right, expanded.top),
    ];
    let transformed = corners.map(|(x, y)| apply_transform(transform, x, y));
    let left = transformed
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::INFINITY, f64::min)
        .floor();
    let right = transformed
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil();
    let top = transformed
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min)
        .floor();
    let bottom = transformed
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil();

    let clamped_left = clamp_pixel(left, page_width);
    let clamped_top = clamp_pixel(top, page_height);
    let clamped_right = clamp_pixel(right, page_width);
    let clamped_bottom = clamp_pixel(bottom, page_height);
    let width = clamped_right.saturating_sub(clamped_left);
    let height = clamped_bottom.saturating_sub(clamped_top);
    if width == 0 || height == 0 {
        return Err(RenderError::invalid_request(
            "Region bounding_box does not intersect the rendered page.",
        ));
    }

    Ok(CropPixels {
        left: clamped_left,
        top: clamped_top,
        width,
        height,
    })
}

fn apply_transform(transform: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (
        transform[0] * x + transform[2] * y + transform[4],
        transform[1] * x + transform[3] * y + transform[5],
    )
}

fn valid_bounding_box(box_: BoundingBox) -> bool {
    box_.left.is_finite()
        && box_.bottom.is_finite()
        && box_.right.is_finite()
        && box_.top.is_finite()
        && box_.right > box_.left
        && box_.top > box_.bottom
}

fn clamp_pixel(value: f64, maximum: u32) -> u32 {
    value.max(0.0).min(f64::from(maximum)) as u32
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

#[cfg(test)]
mod tests {
    use super::*;
    use hayro::hayro_interpret::hayro_syntax::DecryptionError;

    fn fixture_pdf(rotation: i32) -> Vec<u8> {
        fixture_pdf_with_crop_box(rotation, None)
    }

    fn fixture_pdf_with_crop_box(rotation: i32, crop_box: Option<&str>) -> Vec<u8> {
        let content = "0.1 0.2 0.8 rg\n10 10 100 60 re f\n";
        let media_box = if crop_box.is_some() {
            "0 0 200 200"
        } else {
            "0 0 120 80"
        };
        let crop_box = crop_box
            .map(|value| format!(" /CropBox [{value}]"))
            .unwrap_or_default();
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}]{crop_box} /Rotate {rotation} /Resources << >> /Contents 4 0 R >>"
            ),
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ];
        let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn renders_fixture_page_to_bounded_png_at_scale_two() {
        let bytes = fixture_pdf(0);
        let planned = RenderDocument::new(bytes.clone())
            .expect("plan fixture")
            .planned_render_pixel_count(1, 2.0)
            .expect("planned pixels");
        let rendered =
            render_pdf_page(bytes, 1, 2.0, DEFAULT_MAX_RENDER_PIXELS).expect("render fixture");
        assert_eq!(rendered.page, 1);
        assert_eq!(rendered.scale, 2.0);
        assert_eq!((rendered.width, rendered.height), (240, 160));
        assert_eq!(
            rendered.pixel_count,
            u64::from(rendered.width) * u64::from(rendered.height)
        );
        assert_eq!(planned, rendered.pixel_count);
        assert!(rendered.pixel_count <= DEFAULT_MAX_RENDER_PIXELS);
        assert_eq!(&rendered.png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(rendered.provenance.engine, RENDER_ENGINE);
        assert_eq!(rendered.provenance.renderer, RENDERER_NAME);

        let decoded = image::load_from_memory_with_format(&rendered.png, ImageFormat::Png)
            .expect("decode rendered PNG");
        assert_eq!(decoded.dimensions(), (rendered.width, rendered.height));
    }

    #[test]
    fn rejects_invalid_dimensions_and_pixel_budget_before_rendering() {
        let bytes = fixture_pdf(0);
        let err = render_pdf_page(bytes.clone(), 1, 0.0, DEFAULT_MAX_RENDER_PIXELS)
            .expect_err("zero scale");
        assert_eq!(err.code, RenderErrorCode::InvalidRequest);

        let err = render_pdf_page(bytes, 1, 2.0, 1).expect_err("pixel budget");
        assert_eq!(err.code, RenderErrorCode::LimitExceeded);
        assert!(err.message.contains("exceeding max_pixels_per_page"));
    }

    #[test]
    fn reports_page_rotation_and_rotated_dimensions() {
        let rendered = render_pdf_page(fixture_pdf(90), 1, 2.0, DEFAULT_MAX_RENDER_PIXELS)
            .expect("render rotated fixture");
        assert_eq!(rendered.rotation, 90);
        assert_eq!((rendered.width, rendered.height), (160, 240));
    }

    #[test]
    fn reusable_document_reports_pages_and_enforces_selected_page_limit() {
        let document = RenderDocument::new(fixture_pdf(0)).expect("parse fixture once");
        assert_eq!(document.page_count(), 1);
        assert_eq!(pdf_page_count(fixture_pdf(0)).unwrap(), 1);
        let pages = document
            .render_selected_pages(
                &[1],
                1.0,
                RenderLimits {
                    max_pages: 1,
                    ..RenderLimits::default()
                },
            )
            .expect("render selected page");
        assert_eq!(pages.len(), 1);

        let error = document
            .render_selected_pages(
                &[1, 1],
                1.0,
                RenderLimits {
                    max_pages: 1,
                    ..RenderLimits::default()
                },
            )
            .expect_err("selected page count must be bounded");
        assert_eq!(error.code, RenderErrorCode::LimitExceeded);
    }

    #[test]
    fn exact_page_transform_handles_all_right_angle_rotations() {
        let expected = [
            (
                0,
                CropPixels {
                    left: 10,
                    top: 60,
                    width: 20,
                    height: 10,
                },
            ),
            (
                90,
                CropPixels {
                    left: 10,
                    top: 10,
                    width: 10,
                    height: 20,
                },
            ),
            (
                180,
                CropPixels {
                    left: 90,
                    top: 10,
                    width: 20,
                    height: 10,
                },
            ),
            (
                270,
                CropPixels {
                    left: 60,
                    top: 90,
                    width: 10,
                    height: 20,
                },
            ),
        ];
        let region = BoundingBox {
            left: 10.0,
            bottom: 10.0,
            right: 30.0,
            top: 20.0,
        };
        for (rotation, expected_crop) in expected {
            let rendered =
                render_pdf_page(fixture_pdf(rotation), 1, 1.0, DEFAULT_MAX_RENDER_PIXELS).unwrap();
            assert_eq!(
                crop_pixels_for_rendered_page(&rendered, region, 0.0).unwrap(),
                expected_crop,
                "rotation {rotation}"
            );
        }
    }

    #[test]
    fn exact_page_transform_accounts_for_nonzero_crop_box_origin() {
        let rendered = render_pdf_page(
            fixture_pdf_with_crop_box(0, Some("50 60 170 140")),
            1,
            1.0,
            DEFAULT_MAX_RENDER_PIXELS,
        )
        .expect("render cropped page");
        assert_eq!((rendered.width, rendered.height), (120, 80));
        assert_eq!(
            crop_pixels_for_rendered_page(
                &rendered,
                BoundingBox {
                    left: 60.0,
                    bottom: 70.0,
                    right: 80.0,
                    top: 90.0,
                },
                0.0,
            )
            .unwrap(),
            CropPixels {
                left: 10,
                top: 50,
                width: 20,
                height: 20,
            }
        );
    }

    #[test]
    fn malformed_pdf_and_output_limit_return_structured_errors_without_panics() {
        let malformed = render_pdf_page(b"not a PDF".to_vec(), 1, 1.0, DEFAULT_MAX_RENDER_PIXELS)
            .expect_err("malformed input");
        assert_eq!(malformed.code, RenderErrorCode::InvalidPdf);

        let document = RenderDocument::new(fixture_pdf(0)).unwrap();
        let limited = document
            .render_page(1, 1.0, DEFAULT_MAX_RENDER_PIXELS, 16)
            .expect_err("encoded output limit");
        assert_eq!(limited.code, RenderErrorCode::LimitExceeded);
    }

    #[test]
    fn encrypted_pdf_load_errors_are_classified_without_exposing_parser_details() {
        let error = map_load_error(LoadPdfError::Decryption(DecryptionError::PasswordProtected));
        assert_eq!(error.code, RenderErrorCode::EncryptedPdf);
        assert!(error.message.contains("Encrypted PDF"));
    }

    #[test]
    fn rendered_and_cropped_pngs_are_deterministic_rgba8() {
        let rendered_a =
            render_pdf_page(fixture_pdf(0), 1, 1.0, DEFAULT_MAX_RENDER_PIXELS).unwrap();
        let rendered_b =
            render_pdf_page(fixture_pdf(0), 1, 1.0, DEFAULT_MAX_RENDER_PIXELS).unwrap();
        assert_eq!(rendered_a.png, rendered_b.png);
        assert_eq!(
            image::load_from_memory_with_format(&rendered_a.png, ImageFormat::Png)
                .unwrap()
                .color(),
            image::ColorType::Rgba8
        );

        let (crop, crop_png) = crop_rendered_page_region(
            &rendered_a,
            BoundingBox {
                left: 10.0,
                bottom: 10.0,
                right: 30.0,
                top: 20.0,
            },
            0.0,
            DEFAULT_MAX_RENDER_OUTPUT_BYTES,
        )
        .unwrap();
        let decoded = image::load_from_memory_with_format(&crop_png, ImageFormat::Png).unwrap();
        assert_eq!(decoded.dimensions(), (crop.width, crop.height));
        assert_eq!(decoded.color(), image::ColorType::Rgba8);
    }

    #[test]
    fn matches_ts_bottom_left_to_top_left_crop_math_with_clamp_and_padding() {
        assert_eq!(
            crop_pixels_for_bounding_box(
                10,
                10,
                1.0,
                0,
                BoundingBox {
                    left: 2.0,
                    bottom: 2.0,
                    right: 6.0,
                    top: 6.0
                },
                1.0,
            )
            .expect("crop"),
            CropPixels {
                left: 1,
                top: 3,
                width: 6,
                height: 6
            }
        );
        assert_eq!(
            crop_pixels_for_bounding_box(
                10,
                10,
                1.0,
                0,
                BoundingBox {
                    left: -4.0,
                    bottom: -3.0,
                    right: 3.0,
                    top: 4.0
                },
                0.0,
            )
            .expect("clamped crop"),
            CropPixels {
                left: 0,
                top: 6,
                width: 3,
                height: 4
            }
        );
    }

    #[test]
    fn rejects_invalid_or_non_intersecting_bounding_boxes() {
        let invalid = crop_pixels_for_bounding_box(
            10,
            10,
            1.0,
            0,
            BoundingBox {
                left: 4.0,
                bottom: 2.0,
                right: 4.0,
                top: 6.0,
            },
            0.0,
        )
        .expect_err("invalid box");
        assert_eq!(invalid.code, RenderErrorCode::InvalidRequest);

        let outside = crop_pixels_for_bounding_box(
            10,
            10,
            1.0,
            0,
            BoundingBox {
                left: 20.0,
                bottom: 20.0,
                right: 30.0,
                top: 30.0,
            },
            0.0,
        )
        .expect_err("non-intersecting box");
        assert!(outside.message.contains("does not intersect"));

        let rotated = crop_pixels_for_bounding_box(
            80,
            120,
            1.0,
            90,
            BoundingBox {
                left: 2.0,
                bottom: 2.0,
                right: 6.0,
                top: 6.0,
            },
            0.0,
        )
        .expect("rotated crop");
        assert_eq!(
            rotated,
            CropPixels {
                left: 2,
                top: 2,
                width: 4,
                height: 4,
            }
        );
    }

    #[test]
    fn crops_fixture_render_to_valid_png_with_deterministic_dimensions() {
        let rendered = render_pdf_page(fixture_pdf(0), 1, 2.0, DEFAULT_MAX_RENDER_PIXELS)
            .expect("render fixture");
        let crop = CropPixels {
            left: rendered.width / 4,
            top: rendered.height / 4,
            width: rendered.width / 2,
            height: rendered.height / 2,
        };
        let png = crop_rendered_page_png(&rendered.png, rendered.width, rendered.height, crop)
            .expect("crop PNG");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let decoded =
            image::load_from_memory_with_format(&png, ImageFormat::Png).expect("decode crop PNG");
        assert_eq!(decoded.dimensions(), (crop.width, crop.height));
    }
}
