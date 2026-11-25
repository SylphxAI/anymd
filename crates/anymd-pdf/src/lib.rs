//! Clean Markdown from PDF glyph geometry.
//!
//! The pipeline is: glyphs (from `pdf-extract`) → rows by baseline → segments
//! split on large horizontal gaps, with inter-word spaces inferred from glyph
//! gaps → reading order by a column-aware XY cut → paragraphs, headings, list
//! items, and pipe tables → one Markdown string per page.
//!
//! Pages are extracted in parallel and each page is isolated: a page that
//! fails to parse becomes a marker comment instead of failing the document.

mod blocks;
mod convert;
mod error;
mod extract;
mod images;
mod load;
mod margins;
mod metadata;
mod ocr;
mod reading;
mod render;
mod rows;
mod scanned;
mod tables;
#[cfg(test)]
mod tests;
mod types;

pub use crate::convert::{pdf_images, pdf_to_markdown, pdf_to_markdown_with_images};
pub use crate::error::LayoutError;
pub use crate::images::{
    repeated_images, EncodedImage, ImageOptions, PageImage, Placed, MAX_PIXELS, MIN_SIDE_PX,
    SPARSE_PAGE_CHARS,
};
pub(crate) use crate::load::give_back_freed_memory;
pub use crate::load::{load_document, load_document_bytes, page_count};
pub use crate::metadata::{info_title, outline};
pub use crate::ocr::{words_to_markdown, PlacedWord};
pub use crate::rows::{infer_word_spaces, SpacingGlyph};
#[cfg(test)]
use crate::scanned::{image_only_page, scanned_page};
pub use crate::scanned::{image_only_pages, scanned_pages};
pub use crate::types::{MarkdownDocument, MarkdownPage, PdfDocument};

pub(crate) const MAX_GLYPHS_PER_PAGE: usize = 400_000;
pub(crate) const MAX_WORKERS: usize = 8;
