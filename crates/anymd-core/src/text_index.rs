//! Literal PDF text indexing and search for anymd.

use std::collections::BTreeMap;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use pdf_extract::{output_doc, Document};

pub mod cite_check;
mod content_streams;
mod info;
mod search;
mod segments;
mod types;

use content_streams::{validate_page_content_streams, validate_selected_page_content_streams};
pub(crate) use info::read_pdf_info;
pub use search::*;
use segments::{normalize_page_text_parts, TextItemOutput};
pub use types::*;

// Per-page extraction budgets: each page gets its own allowance, so dense
// multi-page documents are not rejected just because their cumulative text
// is large. A separate document-wide backstop below still bounds total work.
const MAX_EXTRACTED_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_GEOMETRY_CHARS: usize = 250_000;
// Document-wide backstop across all pages of one extraction request: 8x the
// per-page allowance, enough for ~1000 pages of average density. Keeps
// pathological documents fail-closed without capping ordinary large PDFs.
const MAX_TOTAL_EXTRACTED_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_GEOMETRY_CHARS: usize = 2_000_000;
const MAX_RAW_TEXT_PARTS: usize = 65_536;
const MAX_RAW_TEXT_PARTS_PER_PAGE: usize = 8_192;
const MAX_NORMALIZED_TEXT_SEGMENTS: usize = 65_536;
const MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE: usize = 8_192;
const TEXT_SEGMENT_GAP_THRESHOLD: f64 = 48.0;

fn validate_pdf_path(path: &Path, max_file_bytes: u64) -> Result<(), TextIndexError> {
    let meta = fs::metadata(path).map_err(|err| {
        TextIndexError::invalid_request(format!(
            "Unable to access file at '{}': {err}",
            path.display()
        ))
    })?;

    if !meta.is_file() {
        return Err(TextIndexError::invalid_request(format!(
            "Path '{}' is not a regular file.",
            path.display()
        )));
    }

    if meta.len() > max_file_bytes {
        return Err(TextIndexError::invalid_request(format!(
            "File exceeds maximum size of {} bytes.",
            max_file_bytes
        )));
    }

    Ok(())
}

/// Extract only the bounded physical pages requested by cite-check. A page
/// failure stays local so supported matches on other pages remain useful.
pub fn extract_cite_pages(
    path: &Path,
    max_file_bytes: u64,
    pages: &[u32],
) -> Result<BTreeMap<u32, Result<ExtractedPageText, TextIndexError>>, TextIndexError> {
    if pages.is_empty() || pages.len() > 20 || pages.contains(&0) {
        return Err(TextIndexError::invalid_params(
            "cite_check needs 1–20 physical pages.",
        ));
    }
    validate_pdf_path(path, max_file_bytes)?;
    let mut doc =
        Document::load(path).map_err(|e| TextIndexError::extraction_failed(e.to_string()))?;
    if doc.is_encrypted() {
        doc.decrypt("")
            .map_err(|e| TextIndexError::extraction_failed(e.to_string()))?;
    }
    let mut segment_count = 0;
    Ok(pages
        .iter()
        .map(|&page| {
            let extracted = (|| {
                validate_selected_page_content_streams(&doc, Some(&[page]))?;
                let mut output = TextItemOutput::default();
                catch_unwind(AssertUnwindSafe(|| {
                    pdf_extract::output_doc_page(&doc, &mut output, page)
                }))
                .map_err(|_| TextIndexError::extraction_failed("Malformed PDF page."))?
                .map_err(|e| TextIndexError::extraction_failed(e.to_string()))?;
                let parts = output
                    .pages
                    .pop()
                    .ok_or_else(|| TextIndexError::extraction_failed("Missing physical page."))?;
                let positioned_items = normalize_page_text_parts(parts, &mut segment_count)?;
                let items = positioned_items
                    .iter()
                    .map(|i| i.text.clone())
                    .collect::<Vec<_>>();
                Ok(ExtractedPageText {
                    text: items.join("\n"),
                    items,
                    positioned_items,
                })
            })();
            (page, extracted)
        })
        .collect())
}

pub fn extract_pdf_text(
    path: &Path,
    max_file_bytes: u64,
) -> Result<ExtractedPdfText, TextIndexError> {
    validate_pdf_path(path, max_file_bytes)?;

    let mut doc = Document::load(path).map_err(|err| {
        TextIndexError::extraction_failed(format!("Failed to extract PDF text: {err}"))
    })?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            TextIndexError::extraction_failed(format!("Failed to extract PDF text: {err}"))
        })?;
    }
    extract_pdf_text_from_document(&doc)
}

pub(crate) fn extract_pdf_text_from_document(
    doc: &Document,
) -> Result<ExtractedPdfText, TextIndexError> {
    let info = read_pdf_info(doc);
    extract_output_bounded(doc, info)
}

/// `pdf-extract` transitively unwraps malformed-font parsing results (including
/// CFF Custom encodings), so an upstream panic must be contained at this
/// extraction boundary and reported like any other extraction failure.
///
/// The same boundary also pre-validates content streams before `output_doc`
/// runs: lopdf 0.42's inline-image parser `unwrap()`s a missing `/CS` entry
/// (`parser/mod.rs`, `image_data_stream`), so a `BI ... ID ... EI` construct
/// without a colorspace (and not an image mask) panics inside
/// `Content::decode` instead of returning `Parse(InvalidContentStream)`
/// (SylphxAI/pdf-reader-mcp#675). Pre-validation computes the expected inline
/// image data length from the inline dict (`/W`, `/H`, `/BPC`, `/CS`, with
/// `/Width`, `/Height`, `/BitsPerComponent`, `/ColorSpace` and `/IM`,
/// `/ImageMask`, `/G`, `/RGB`, `/CMYK` abbreviations supported) and verifies
/// `EI` follows, rather than scanning for a literal `EI` that can match
/// inside image data. Any page whose content stream fails validation is
/// reported as a page-level tool error naming the page, never a panic.
fn extract_output_bounded(
    doc: &Document,
    info: PdfInfo,
) -> Result<ExtractedPdfText, TextIndexError> {
    validate_page_content_streams(doc)?;
    let mut output = TextItemOutput::default();
    // Defense in depth: pre-validation above rejects the known malformed
    // inline-image shapes, but any other upstream parse panic (CFF fonts,
    // future lopdf paths) must still surface as a structured tool error.
    catch_unwind(AssertUnwindSafe(|| output_doc(doc, &mut output)))
        .map_err(|_| {
            TextIndexError::extraction_failed("Failed to extract PDF text: malformed font encoding")
        })?
        .map_err(|err| {
            TextIndexError::extraction_failed(format!("Failed to extract PDF text: {err}"))
        })?;

    let pages = if output.pages.is_empty() {
        vec![ExtractedPageText {
            text: String::new(),
            items: Vec::new(),
            positioned_items: Vec::new(),
        }]
    } else {
        let mut request_segment_count = 0usize;
        output
            .pages
            .into_iter()
            .map(|parts| {
                let positioned_items =
                    normalize_page_text_parts(parts, &mut request_segment_count)?;
                let items = positioned_items
                    .iter()
                    .map(|item| item.text.clone())
                    .collect::<Vec<_>>();
                // One line per positioned item, like the document text layer;
                // concatenating them glued words across line ends.
                Ok(ExtractedPageText {
                    text: items.join("\n"),
                    items,
                    positioned_items,
                })
            })
            .collect::<Result<Vec<_>, TextIndexError>>()?
    };

    Ok(ExtractedPdfText { pages, info })
}

pub fn extract_page_texts(path: &Path, max_file_bytes: u64) -> Result<Vec<String>, TextIndexError> {
    let extracted = extract_pdf_text(path, max_file_bytes)?;

    Ok(extracted.pages.into_iter().map(|page| page.text).collect())
}

#[cfg(test)]
mod tests;
