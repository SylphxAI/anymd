//! Bounded, pure-Rust PDF page rendering and region cropping primitives.

use serde::{Deserialize, Serialize};

mod document;
mod error;
mod geometry;
mod limits;
mod page;
mod png;
#[cfg(test)]
mod tests;

#[cfg(test)]
use self::document::map_load_error;
#[cfg(test)]
use hayro::hayro_interpret::hayro_syntax::LoadPdfError;

pub use document::{pdf_page_count, render_pdf_page, RenderDocument};
pub use error::{RenderError, RenderErrorCode};
pub use geometry::{
    crop_pixels_for_bounding_box, crop_pixels_for_rendered_page, pdf_box_from_pixel_box,
    BoundingBox, CropPixels,
};
pub use limits::RenderLimits;
pub use png::{
    crop_rendered_page_png, crop_rendered_page_png_with_limit, crop_rendered_page_region,
};

pub const DEFAULT_RENDER_SCALE: f32 = 2.0;
pub const DEFAULT_MAX_RENDER_PIXELS: u64 = 16_000_000;
pub const DEFAULT_MAX_RENDER_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_MAX_RENDER_PAGES: usize = 20;
pub const RENDER_ENGINE: &str = "hayro";
pub const RENDERER_NAME: &str = "hayro/vello_cpu";

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
