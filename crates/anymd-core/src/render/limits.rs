//! Render budgets and request validation.

use super::{
    RenderError, DEFAULT_MAX_RENDER_OUTPUT_BYTES, DEFAULT_MAX_RENDER_PAGES,
    DEFAULT_MAX_RENDER_PIXELS, DEFAULT_RENDER_SCALE,
};

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

pub(super) fn validate_render_request(
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

pub(super) fn validate_limits(limits: RenderLimits) -> Result<(), RenderError> {
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
