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
mod extract;
mod margins;
mod metadata;
mod reading;
mod render;
mod rows;
mod tables;
#[cfg(test)]
mod tests;

use std::path::Path;

use pdf_extract::Document;

use crate::blocks::{layout_page, Block};
use crate::extract::extract_pages;
use crate::margins::repeated_margin_lines;
use crate::render::{heading_levels, is_size_heading, render_blocks};
use crate::rows::body_font_size;
pub use crate::metadata::{info_title, outline};
pub use crate::rows::{infer_word_spaces, SpacingGlyph};

/// A PDF that could not be opened or converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutError {
    pub message: String,
}

impl LayoutError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LayoutError {}

pub(crate) const MAX_GLYPHS_PER_PAGE: usize = 400_000;
pub(crate) const MAX_WORKERS: usize = 8;

/// A parsed PDF (lopdf document).
pub type PdfDocument = Document;

/// One page of converted Markdown.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownPage {
    /// 1-based page number.
    pub number: u32,
    pub markdown: String,
}

/// A converted PDF (or the requested subset of its pages).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownDocument {
    pub title: Option<String>,
    pub page_count: u32,
    pub pages: Vec<MarkdownPage>,
    /// Bookmarks as (depth, title, page), depth 0 = top level.
    pub outline: Vec<(usize, String, Option<u32>)>,
}

/// Open a PDF from disk (decrypting with the empty password when needed).
pub fn load_document(path: &Path) -> Result<Document, LayoutError> {
    let mut doc = Document::load(path)
        .map_err(|err| LayoutError::new(format!("Failed to open PDF: {err}")))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            LayoutError::new(format!(
                "PDF is encrypted and needs a password: {err}"
            ))
        })?;
    }
    Ok(doc)
}

/// Open a PDF from memory (decrypting with the empty password when needed).
pub fn load_document_bytes(bytes: &[u8]) -> Result<Document, LayoutError> {
    let mut doc = Document::load_mem(bytes)
        .map_err(|err| LayoutError::new(format!("Failed to open PDF: {err}")))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            LayoutError::new(format!(
                "PDF is encrypted and needs a password: {err}"
            ))
        })?;
    }
    Ok(doc)
}

/// Page count without extracting text.
pub fn page_count(doc: &Document) -> u32 {
    u32::try_from(doc.get_pages().len()).unwrap_or(u32::MAX)
}

/// Convert the selected pages (1-based; `None` = all) of a PDF to Markdown.
pub fn pdf_to_markdown(
    doc: &Document,
    pages: Option<&[u32]>,
) -> Result<MarkdownDocument, LayoutError> {
    let page_map = doc.get_pages();
    let total = u32::try_from(page_map.len()).unwrap_or(u32::MAX);
    let selected: Vec<u32> = match pages {
        Some(list) => list
            .iter()
            .copied()
            .filter(|page| page_map.contains_key(page))
            .collect(),
        None => page_map.keys().copied().collect(),
    };
    let raw = extract_pages(doc, &selected);
    let body_size = body_font_size(&raw);
    let repeated = repeated_margin_lines(&raw);
    let mut heading_sizes = Vec::<f64>::new();
    let mut laid_out = Vec::with_capacity(raw.len());
    for page in &raw {
        let blocks = match &page.glyphs {
            Ok(glyphs) => layout_page(glyphs, page, body_size, &repeated),
            Err(message) => vec![Block::Comment(message.clone())],
        };
        for block in &blocks {
            if let Block::Paragraph { size, text, .. } = block {
                if is_size_heading(*size, body_size, text) {
                    heading_sizes.push(*size);
                }
            }
        }
        laid_out.push((page.number, blocks));
    }
    let levels = heading_levels(&heading_sizes);
    let mut first_heading = None;
    let pages = laid_out
        .into_iter()
        .map(|(number, blocks)| MarkdownPage {
            number,
            markdown: {
                let mut heading = None;
                let markdown = render_blocks(&blocks, body_size, &levels, &mut heading);
                if number == 1 {
                    first_heading = heading;
                }
                markdown
            },
        })
        .collect();
    let title = info_title(doc).or(first_heading);
    Ok(MarkdownDocument {
        title,
        page_count: total,
        pages,
        outline: outline(doc),
    })
}
