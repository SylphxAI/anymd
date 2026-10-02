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
mod images;
mod margins;
mod metadata;
mod ocr;
mod reading;
mod render;
mod rows;
mod tables;
#[cfg(test)]
mod tests;

use std::path::Path;

use pdf_extract::{Document, LoadOptions, Object, ObjectId};

use crate::blocks::{layout_page, Block};
use crate::extract::extract_pages;
pub use crate::images::{
    repeated_images, EncodedImage, ImageOptions, PageImage, Placed, MAX_PIXELS, MIN_SIDE_PX,
    SPARSE_PAGE_CHARS,
};
use crate::margins::repeated_margin_lines;
pub use crate::metadata::{info_title, outline};
pub use crate::ocr::{words_to_markdown, PlacedWord};
use crate::render::{heading_levels, is_size_heading, render_blocks};
use crate::rows::body_font_size;
pub use crate::rows::{infer_word_spaces, SpacingGlyph};

/// A PDF that could not be opened or converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutError {
    pub message: String,
}

impl LayoutError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
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

/// Objects that text layout never reads: link and widget annotations and the
/// tagged-PDF structure tree. A tagged book carries tens of thousands of them
/// (about 130 MB of the 142 MB heap for the 492-page NIST SP 800-53r5), so
/// they are dropped while the file is parsed instead of held until the end.
/// Layout reads page content, fonts, images, the outline and the Info title
/// only; none of those reach these objects.
///
/// lopdf keeps the object in place when it is read directly and takes the
/// returned one when it comes out of an object stream, so a kept object is
/// returned as a copy.
fn keep_for_layout(id: ObjectId, object: &mut Object) -> Option<(ObjectId, Object)> {
    if let Object::Dictionary(dict) = object {
        let name = |key: &[u8]| dict.get(key).ok().and_then(|value| value.as_name().ok());
        let droppable = match name(b"Type") {
            Some(b"Annot") | Some(b"StructElem") | Some(b"MCR") | Some(b"OBJR") => true,
            Some(_) => false,
            // Structure elements and annotations may omit /Type.
            None => {
                (dict.has(b"S") && dict.has(b"P") && (dict.has(b"K") || dict.has(b"Pg")))
                    || (dict.has(b"Subtype") && dict.has(b"Rect"))
            }
        };
        if droppable {
            return None;
        }
    }
    Some((id, object.clone()))
}

fn layout_options() -> LoadOptions {
    LoadOptions::with_filter(keep_for_layout)
}

/// Drop every object that cannot be reached from the trailer. Once the
/// annotations and the structure tree are gone, what only they pointed at
/// (link actions and destinations, the structure parent tree) is dead weight
/// too, and the page tree, fonts, images and outline are all reached from the
/// trailer, so layout never looks for anything else. A trailer without a
/// reachable `/Root` leaves the document as it is.
fn prune_unreachable(doc: &mut Document) {
    fn visit<'a>(object: &'a Object, stack: &mut Vec<ObjectId>) {
        match object {
            Object::Reference(id) => stack.push(*id),
            Object::Array(items) => items.iter().for_each(|item| visit(item, stack)),
            Object::Dictionary(dict) => dict.iter().for_each(|(_, value)| visit(value, stack)),
            Object::Stream(stream) => stream.dict.iter().for_each(|(_, value)| visit(value, stack)),
            _ => {}
        }
    }
    if !matches!(doc.trailer.get(b"Root"), Ok(Object::Reference(_))) {
        return;
    }
    let mut stack = Vec::new();
    doc.trailer.iter().for_each(|(_, value)| visit(value, &mut stack));
    let mut reached = std::collections::HashSet::new();
    while let Some(id) = stack.pop() {
        if !reached.insert(id) {
            continue;
        }
        if let Some(object) = doc.objects.get(&id) {
            visit(object, &mut stack);
        }
    }
    doc.objects.retain(|id, _| reached.contains(id));
    give_back_freed_memory();
}

/// Parsing a document, and extracting a chunk of pages on worker threads,
/// peak well above what is kept afterwards; glibc holds the freed pages in
/// per-thread arenas for reuse, so hand them back and the process stays at
/// the size of what it actually holds.
pub(crate) fn give_back_freed_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` takes no pointers and only releases free heap pages.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Open a PDF from disk (decrypting with the empty password when needed).
pub fn load_document(path: &Path) -> Result<Document, LayoutError> {
    let mut doc = Document::load_with_options(path, layout_options())
        .map_err(|err| LayoutError::new(format!("Failed to open PDF: {err}")))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            LayoutError::new(format!("PDF is encrypted and needs a password: {err}"))
        })?;
    }
    prune_unreachable(&mut doc);
    Ok(doc)
}

/// Open a PDF from memory (decrypting with the empty password when needed).
pub fn load_document_bytes(bytes: &[u8]) -> Result<Document, LayoutError> {
    let mut doc = Document::load_mem_with_options(bytes, layout_options())
        .map_err(|err| LayoutError::new(format!("Failed to open PDF: {err}")))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            LayoutError::new(format!("PDF is encrypted and needs a password: {err}"))
        })?;
    }
    prune_unreachable(&mut doc);
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
    pdf_to_markdown_with_images(doc, pages, None)
}

/// Pages containing painted raster images but no native letters or digits.
///
/// Uses the same glyph and image extraction as native Markdown. A short title
/// or a failed text extraction is not evidence of an image-only page.
pub fn image_only_pages(doc: &Document, pages: &[u32]) -> Vec<u32> {
    extract_pages(doc, pages)
        .into_iter()
        .filter(image_only_page)
        .map(|page| page.number)
        .collect()
}

/// Pages that are image-only AND mostly covered by images: scans, as opposed to
/// a blank page carrying a logo or a spacer image. Image boxes (summed, capped
/// at the page area) must cover at least half the page, so a scan split into
/// strips still counts.
pub fn scanned_pages(doc: &Document, pages: &[u32]) -> Vec<u32> {
    extract_pages(doc, pages)
        .into_iter()
        .filter(scanned_page)
        .map(|page| page.number)
        .collect()
}

fn scanned_page(page: &crate::extract::RawPage) -> bool {
    if !image_only_page(page) || !(page.area > 0.0) {
        return false;
    }
    let covered: f64 = page
        .images
        .iter()
        .map(|image| {
            (image.bbox[2] - image.bbox[0]).max(0.0) * (image.bbox[3] - image.bbox[1]).max(0.0)
        })
        .sum();
    covered.min(page.area) >= 0.5 * page.area
}

fn image_only_page(page: &crate::extract::RawPage) -> bool {
    !page.images.is_empty()
        && page.glyphs.as_ref().is_ok_and(|glyphs| {
            !glyphs
                .iter()
                .chain(&page.rotated)
                .flat_map(|glyph| glyph.text.chars())
                .any(char::is_alphanumeric)
        })
}

/// The selected pages that exist, and the page count.
fn selected_pages(doc: &Document, pages: Option<&[u32]>) -> (Vec<u32>, u32) {
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
    (selected, total)
}

/// The embedded images of the selected pages that matter (see [`ImageOptions`]),
/// exported through `options.place`, with their page, extent and caption.
pub fn pdf_images(
    doc: &Document,
    pages: Option<&[u32]>,
    options: &ImageOptions<'_>,
) -> Vec<PageImage> {
    let (selected, _) = selected_pages(doc, pages);
    extract_pages(doc, &selected)
        .iter()
        .flat_map(|page| crate::images::figures_for_page(doc, page, options))
        .map(|(_, image)| image)
        .collect()
}

/// Like [`pdf_to_markdown`], with embedded images exported and marked where
/// they sit in reading order when `images` is given.
pub fn pdf_to_markdown_with_images(
    doc: &Document,
    pages: Option<&[u32]>,
    images: Option<&ImageOptions<'_>>,
) -> Result<MarkdownDocument, LayoutError> {
    let (selected, total) = selected_pages(doc, pages);
    let mut raw = extract_pages(doc, &selected);
    if let Some(options) = images {
        for page in &mut raw {
            page.figures = crate::images::figures_for_page(doc, page, options)
                .into_iter()
                .map(|(figure, _)| figure)
                .collect();
        }
    }
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
