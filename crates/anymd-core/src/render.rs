//! Bounded, pure-Rust PDF page rendering and region cropping primitives.

use std::panic::{catch_unwind, AssertUnwindSafe};

use hayro::hayro_interpret::hayro_syntax::page::Rotation;
use hayro::hayro_interpret::hayro_syntax::{LoadPdfError, Pdf};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{render_into, RenderCache, RenderSettings};
use serde::{Deserialize, Serialize};

mod geometry;
mod png;
#[cfg(test)]
mod tests;

use self::png::encode_rgba_png_bounded;
pub use geometry::{
    crop_pixels_for_bounding_box, crop_pixels_for_rendered_page, pdf_box_from_pixel_box,
    BoundingBox, CropPixels,
};
pub use png::{
    crop_rendered_page_png, crop_rendered_page_png_with_limit, crop_rendered_page_region,
};

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

impl RenderedPage {
    /// The actual Hayro affine transform, including rotation and CropBox origin.
    pub fn pdf_to_pixel_transform(&self) -> [f64; 6] {
        self.pdf_to_pixel
    }
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

        let mut pdf_to_pixel = page.initial_transform(true).as_coeffs();
        for coefficient in &mut pdf_to_pixel {
            *coefficient *= f64::from(scale);
        }

        // hayro's scale-only helper truncates fractional dimensions. Keep the
        // preflight's rounded-up viewport so allocation and PNG dimensions agree.
        let mut context = RenderContext::new(viewport_width, viewport_height);
        let transform = hayro::kurbo::Affine::new(pdf_to_pixel);
        render_into(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &mut context,
            transform,
        );
        context.flush();
        let mut pixmap = Pixmap::new(viewport_width, viewport_height);
        context.render_with(
            &mut pixmap,
            &mut Resources::default(),
            RasterizerSettings {
                target_init: TargetInit::Clear(WHITE),
                ..Default::default()
            },
        );
        let rgba = pixmap.data_as_u8_slice();
        if rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 255) {
            return Err(RenderError::encode_failed(
                "White-background render unexpectedly contained transparent pixels.",
            ));
        }
        let png = encode_rgba_png_bounded(rgba, width, height, max_output_bytes)?;

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
