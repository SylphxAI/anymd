//! Parse PDF documents once and dispatch bounded single- or multi-page renders.

use std::panic::{catch_unwind, AssertUnwindSafe};

use hayro::hayro_interpret::hayro_syntax::{LoadPdfError, Pdf};

use super::limits::{validate_limits, validate_render_request};
use super::page::{planned_dimensions, rasterize_page};
use super::{RenderError, RenderLimits, RenderedPage, DEFAULT_MAX_RENDER_OUTPUT_BYTES};

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
        rasterize_page(page, page_number, scale, max_pixels, max_output_bytes)
    }
}

pub(super) fn map_load_error(error: LoadPdfError) -> RenderError {
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
