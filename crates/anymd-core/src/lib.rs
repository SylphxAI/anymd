//! Rust hashing, SSRF-safe fetch, and text-index primitives for anymd.

mod accessibility;
mod catalog_signals;
mod cos_document;
pub mod document_twin;
mod form_attachment_signals;
mod image_signals;
pub use anymd_pdf as markdown_layout;
pub mod ocr_fusion;
pub mod ocr_tables;
pub mod page_cache;
mod page_signals;
mod pdfjs_text;
pub mod compare_pdf;
pub mod read_pdf;
mod read_result_cache;
pub mod render;
pub mod search_pdf;
pub mod ssrf;
mod structure_signals;
pub mod text_index;
pub mod url_fetch;
pub mod visual_candidates;
pub mod visual_fusion;

pub use ssrf::{assert_host_not_private, is_private_ip};

pub fn inspect_structure_tree_presence(
    path: &Path,
    max_file_bytes: u64,
    selected_pages: &[u32],
) -> Result<(u32, bool), text_index::TextIndexError> {
    let parsed = cos_document::ParsedPdf::load(path, max_file_bytes)?;
    let total = parsed.pages.len().max(1) as u32;
    let trees = structure_signals::extract_structure_trees_checked(
        &parsed.document,
        &parsed.pages,
        selected_pages,
    )
    .map_err(|()| text_index::TextIndexError {
        code: text_index::TextIndexErrorCode::ExtractionFailed,
        message: "Tagged structure inspection exceeded safety limits or is malformed.".into(),
    })?;
    if trees.as_ref().is_some_and(|value| !value.complete) {
        return Err(text_index::TextIndexError {
            code: text_index::TextIndexErrorCode::ExtractionFailed,
            message: "Tagged structure inspection failed for one or more sampled pages.".into(),
        });
    }
    let has_tree = trees.is_some_and(|value| !value.trees.is_empty());
    Ok((total, has_tree))
}

pub fn inspect_document_signal_presence(
    path: &Path,
    max_file_bytes: u64,
    selected_pages: &[u32],
) -> Result<(u32, serde_json::Value), text_index::TextIndexError> {
    let parsed = cos_document::ParsedPdf::load(path, max_file_bytes)?;
    let total = parsed.pages.len().max(1) as u32;
    let catalog = catalog_signals::extract_catalog_signals(
        &parsed.document,
        parsed.encryption_facts,
        total,
        catalog_signals::CatalogSignalRequest {
            page_labels: true,
            permissions: true,
            outline: true,
        },
    );
    let forms = form_attachment_signals::extract_form_attachment_signals(
        &parsed.document,
        &parsed.pages,
        true,
        true,
    );
    let trees = structure_signals::extract_structure_trees_checked(
        &parsed.document,
        &parsed.pages,
        selected_pages,
    )
    .map_err(|()| text_index::TextIndexError {
        code: text_index::TextIndexErrorCode::ExtractionFailed,
        message: "Tagged structure inspection exceeded safety limits or is malformed.".into(),
    })?;
    if trees.as_ref().is_some_and(|value| !value.complete) {
        return Err(text_index::TextIndexError {
            code: text_index::TextIndexErrorCode::ExtractionFailed,
            message: "Tagged structure inspection failed for one or more sampled pages.".into(),
        });
    }
    Ok((
        total,
        serde_json::json!({
            "has_outline": catalog.outline.as_ref().is_some_and(|value| !value.is_empty()),
            "has_page_labels": catalog.page_labels.as_ref().is_some_and(|value| !value.is_empty()),
            "has_permissions": catalog.permissions.as_ref().is_some_and(|value| !value.is_empty()),
            "has_mark_info": catalog.mark_info.is_some(),
            "has_form_fields": forms.form_fields.as_ref().is_some_and(|value| !value.is_empty()),
            "has_attachments": forms.attachments.as_ref().is_some_and(|value| !value.is_empty()),
            "has_structure_tree": trees.is_some_and(|value| !value.trees.is_empty()),
        }),
    ))
}

pub use ocr_fusion::{fuse_ocr_outcomes, OcrPage, OcrWord, SourceOcrOutcome, OCR_STUB_WARNING};
pub use compare_pdf::{compare_pdf, compare_pdf_from_value, ComparePdfError, ComparePdfInput, ComparePdfResponse};
pub use read_pdf::{
    read_pdf, read_pdf_from_value, ReadPdfError, ReadPdfErrorCode, ReadPdfInput, ReadPdfResponse,
    ReadPdfSource, ReadPdfSourceResult, READ_PDF_ROUTE,
};
pub use render::{
    crop_pixels_for_bounding_box, crop_pixels_for_rendered_page, crop_rendered_page_png,
    crop_rendered_page_png_with_limit, crop_rendered_page_region, pdf_page_count, render_pdf_page,
    BoundingBox, CropPixels, RenderDocument, RenderError, RenderErrorCode, RenderLimits,
    RenderProvenance, RenderedPage, DEFAULT_MAX_RENDER_OUTPUT_BYTES, DEFAULT_MAX_RENDER_PAGES,
    DEFAULT_MAX_RENDER_PIXELS, DEFAULT_RENDER_SCALE, RENDERER_NAME, RENDER_ENGINE,
};
pub use search_pdf::{
    fuse_search_ocr_outcomes, search_pdf, search_pdf_from_value, SearchPdfError,
    SearchPdfErrorCode, SearchPdfInput, SearchPdfResponse, SearchPdfSource, SEARCH_PDF_ROUTE,
};
pub use visual_fusion::{
    fuse_visual_outcomes, SourceVisualOutcome, VISUAL_NO_CANDIDATE_WARNING, VISUAL_STUB_WARNING,
};

pub use page_cache::{
    extract_page_texts_cached, load_cached_pages, page_cache_path, store_cached_pages,
    PageCacheStatus, PageTextCacheEntry, PAGE_CACHE_DIR, PAGE_CACHE_SCHEMA_VERSION,
};

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ENGINE_NAME: &str = "pdf-reader-core";
pub const ENGINE_VERSION: &str = "3.1.1";
pub const HASH_ROUTE: &str = "rust-sha256";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHash {
    pub path: String,
    pub source_hash: String,
    pub file_size: u64,
    pub route: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashErrorCode {
    InvalidParams,
    InvalidRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashError {
    pub code: HashErrorCode,
    pub message: String,
}

impl HashError {
    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: HashErrorCode::InvalidRequest,
            message: message.into(),
        }
    }
}

pub fn hash_file(path: &Path, max_file_bytes: u64) -> Result<FileHash, HashError> {
    let meta = fs::metadata(path).map_err(|err| {
        HashError::invalid_request(format!(
            "Unable to access file at '{}': {err}",
            path.display()
        ))
    })?;

    if !meta.is_file() {
        return Err(HashError::invalid_request(format!(
            "Path '{}' is not a regular file.",
            path.display()
        )));
    }

    let file_size = meta.len();
    if file_size > max_file_bytes {
        return Err(HashError::invalid_request(format!(
            "File exceeds maximum size of {} bytes.",
            max_file_bytes
        )));
    }

    let bytes = fs::read(path)
        .map_err(|err| HashError::invalid_request(format!("Failed to read file bytes: {err}")))?;

    Ok(FileHash {
        path: path.to_string_lossy().to_string(),
        source_hash: format!("{:x}", Sha256::digest(&bytes)),
        file_size,
        route: HASH_ROUTE.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hashes_fixture_bytes_deterministically() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.pdf");
        let mut file = fs::File::create(&path).expect("create");
        file.write_all(b"%PDF-1.4 sample").expect("write");

        let first = hash_file(&path, 32 * 1024 * 1024).expect("hash");
        let second = hash_file(&path, 32 * 1024 * 1024).expect("hash");
        assert_eq!(first.source_hash, second.source_hash);
        assert_eq!(first.route, HASH_ROUTE);
        assert_eq!(first.file_size, 15);
    }

    #[test]
    fn rejects_oversized_files_before_hashing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("big.pdf");
        let mut file = fs::File::create(&path).expect("create");
        file.write_all(&[0u8; 64]).expect("write");
        let err = hash_file(&path, 32).expect_err("oversized");
        assert_eq!(err.code, HashErrorCode::InvalidRequest);
    }

    #[test]
    fn bulk_hash_rejects_oversize_and_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("big.bin");
        std::fs::write(&path, b"0123456789").expect("write");
        let err = hash_file(&path, 5).expect_err("oversize");
        assert_eq!(err.code, HashErrorCode::InvalidRequest);
        let missing = hash_file(&temp.path().join("nope"), 100).expect_err("missing");
        assert_eq!(missing.code, HashErrorCode::InvalidRequest);
    }
}
