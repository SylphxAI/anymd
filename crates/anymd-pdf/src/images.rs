//! Embedded raster images: which ones matter, where they sit, what they are
//! captioned, and the bytes to export.
//!
//! An image XObject is exported only when it can carry content: at least
//! 48 x 48 px, at least 2% of the page, and not the same picture on three or
//! more pages (a logo or a running header). JPEG data is exported as it is;
//! 8-bit gray, RGB and palette rasters are re-encoded as PNG. Other encodings
//! (CMYK, JPEG 2000, JBIG2, CCITT, 1-bit masks) are skipped: vector figures
//! and those encodings are not covered yet.

mod caption;
mod encode;
mod figures;
mod objects;

use std::collections::HashSet;

pub use figures::repeated_images;
pub(crate) use figures::{figures_for_page, is_scan_like};
pub(crate) use objects::{dims, image_stream};

/// Images with more pixels than this are refused, before any decoding.
pub const MAX_PIXELS: u64 = 50_000_000;
/// Images narrower or shorter than this are decoration.
pub const MIN_SIDE_PX: u32 = 48;
/// A page with fewer letters and digits than this has no usable text layer:
/// it is sent to OCR, and an image over it is a scan, not a figure.
pub const SPARSE_PAGE_CHARS: usize = 24;

/// An image ready to be written to a file.
#[derive(Debug, Clone)]
pub struct EncodedImage {
    pub bytes: Vec<u8>,
    /// File extension without the dot: "jpg" or "png".
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
}

/// An image written by the caller's `place` function.
#[derive(Debug, Clone)]
pub struct Placed {
    /// Absolute path of the exported file.
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// The Markdown that points at it.
    pub markdown: String,
}

/// How embedded images are exported. `place` receives the encoded image, its
/// caption when the page has one, and the page number; it writes the file and
/// returns the Markdown, or `None` to leave the image out.
pub type PlaceImage<'a> = dyn Fn(&EncodedImage, Option<&str>, u32) -> Option<Placed> + 'a;

pub struct ImageOptions<'a> {
    /// Images that repeat across the document (see [`repeated_images`]).
    pub repeated: &'a HashSet<u64>,
    pub place: &'a PlaceImage<'a>,
}

/// An exported image and where it sits on its page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageImage {
    pub page: u32,
    /// Page space, points, y up: x0, y0, x1, y1.
    pub bbox: [f64; 4],
    pub width: u32,
    pub height: u32,
    pub caption: Option<String>,
    pub path: String,
}

/// An exported image, placed in the page's reading order.
#[derive(Debug, Clone)]
pub(crate) struct Figure {
    pub(crate) bbox: [f64; 4],
    pub(crate) markdown: String,
}

/// "Figure 3", "Fig. 3", "Table 2", "圖1", "图 1", "表3" at the start of a line.
pub(crate) fn is_caption_start(text: &str) -> bool {
    let lower = text.trim_start().to_lowercase();
    let rest = ["figure", "fig.", "fig", "table", "圖", "图", "表"]
        .iter()
        .find_map(|prefix| lower.strip_prefix(*prefix));
    let Some(rest) = rest else {
        return false;
    };
    rest.trim_start()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || ('０'..='９').contains(&c))
}
