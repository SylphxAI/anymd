//! TS-compatible page render and region crop orchestration over the pure-Rust kernel.

mod ocr_render;
mod page_render;
mod regions;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};

use anymd_core::url_fetch::{cleanup_temp_file, fetch_url_to_temp_file};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::Value;

use crate::schema::{PdfEvidenceArgs, PdfEvidenceSource, PdfSource};

pub(crate) use ocr_render::{render_ocr_source, OcrRenderRequest};
#[allow(
    unused_imports,
    reason = "retain the original crate-internal type paths"
)]
pub(crate) use ocr_render::{RenderedOcrPage, RenderedOcrSource};
pub use page_render::render_pages;
pub use regions::extract_regions;

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_MAX_RENDER_PAGES: usize = 5;
const DEFAULT_MAX_REGIONS: usize = 20;
const MAX_AGGREGATE_IMAGE_BYTES: usize = 256 * 1024 * 1024;
const MAX_SOURCES_PER_REQUEST: usize = 32;
const MAX_REQUEST_RENDERED_PAGES: usize = 64;
const MAX_REQUEST_REGIONS: usize = 200;
const MAX_REQUEST_RENDER_PIXELS: u64 = 256_000_000;

#[derive(Default)]
pub(crate) struct RequestWorkBudget {
    pub(crate) rendered_pages: usize,
    regions: usize,
    pub(crate) render_pixels: u64,
    encoded_frame_bytes: u64,
    exhausted: Option<String>,
}

impl RequestWorkBudget {
    pub(crate) fn ensure_available(&self) -> Result<(), String> {
        self.exhausted.clone().map_or(Ok(()), Err)
    }

    pub(crate) fn exhaust(&mut self, message: String) -> Result<(), String> {
        self.exhausted = Some(message.clone());
        Err(message)
    }

    pub(crate) fn charge_page(&mut self, pixels: u64) -> Result<(), String> {
        self.ensure_available()?;
        let Some(rendered_pages) = self.rendered_pages.checked_add(1) else {
            return self.exhaust("Request render page work overflow.".to_string());
        };
        if rendered_pages > MAX_REQUEST_RENDERED_PAGES {
            return self.exhaust(format!(
                "Request exceeds render work limit of {MAX_REQUEST_RENDERED_PAGES} pages."
            ));
        }
        let Some(render_pixels) = self.render_pixels.checked_add(pixels) else {
            return self.exhaust("Request render pixel work overflow.".to_string());
        };
        if render_pixels > MAX_REQUEST_RENDER_PIXELS {
            return self.exhaust(format!(
                "Request exceeds render work limit of {MAX_REQUEST_RENDER_PIXELS} pixels."
            ));
        }
        self.rendered_pages = rendered_pages;
        self.render_pixels = render_pixels;
        Ok(())
    }

    /// Lower operation-specific bounds reuse this request's sticky work state.
    pub(crate) fn charge_frame(&mut self, pixels: u64, bytes: u64) -> Result<(), String> {
        self.charge_page(pixels)?;
        self.encoded_frame_bytes = self.encoded_frame_bytes.saturating_add(bytes);
        if self.render_pixels > 40_000_000 || self.encoded_frame_bytes > 32 * 1024 * 1024 {
            return self.exhaust("video aggregate frame budget exceeded".into());
        }
        Ok(())
    }

    fn charge_region(&mut self) -> Result<(), String> {
        self.ensure_available()?;
        let Some(regions) = self.regions.checked_add(1) else {
            return self.exhaust("Request region work overflow.".to_string());
        };
        if regions > MAX_REQUEST_REGIONS {
            return self.exhaust(format!(
                "Request exceeds crop work limit of {MAX_REQUEST_REGIONS} regions."
            ));
        }
        self.regions = regions;
        Ok(())
    }
}

pub(crate) struct MaterializedSource {
    source_index: usize,
    label: String,
    path: PathBuf,
    temporary: bool,
}

impl MaterializedSource {
    pub(crate) fn source_index(&self) -> usize {
        self.source_index
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for MaterializedSource {
    fn drop(&mut self) {
        if self.temporary {
            cleanup_temp_file(&self.path);
        }
    }
}

struct ImagePart {
    data: String,
    mime_type: &'static str,
}

fn tool_result(payload: Value, images: Vec<ImagePart>) -> CallToolResult {
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
    let mut content = Vec::with_capacity(images.len() + 1);
    content.push(ContentBlock::text(text));
    content.extend(
        images
            .into_iter()
            .map(|image| ContentBlock::image(image.data, image.mime_type)),
    );
    {
        let mut result = CallToolResult::structured(payload);
        result.content = content;
        result
    }
}

fn all_failed(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

fn parse_args(value: Value) -> Result<PdfEvidenceArgs, rmcp::ErrorData> {
    let args: PdfEvidenceArgs = serde_json::from_value(value).map_err(|error| {
        rmcp::ErrorData::invalid_params(format!("Invalid pdf_evidence arguments: {error}"), None)
    })?;
    args.validate()
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    if args.sources.len() > MAX_SOURCES_PER_REQUEST {
        return Err(rmcp::ErrorData::invalid_params(
            format!("pdf_evidence accepts at most {MAX_SOURCES_PER_REQUEST} sources per request."),
            None,
        ));
    }
    Ok(args)
}

fn materialize(source: &PdfEvidenceSource) -> Result<MaterializedSource, String> {
    if let Some(path) = source.path.as_ref() {
        return Ok(MaterializedSource {
            source_index: 0,
            label: path.clone(),
            path: PathBuf::from(path),
            temporary: false,
        });
    }
    if let Some(url) = source.url.as_ref() {
        return fetch_url_to_temp_file(url).map(|path| MaterializedSource {
            source_index: 0,
            label: url.clone(),
            path,
            temporary: true,
        });
    }
    Err("Provide exactly one of path or url for each PDF source.".into())
}

/// Resolve each read source exactly once. URL-backed temporary files live until
/// the returned owner is dropped, allowing extraction and OCR rendering to
/// share the same bytes without a second fetch.
pub(crate) fn materialize_read_source(
    source_index: usize,
    source: &PdfSource,
) -> Result<MaterializedSource, String> {
    if let Some(path) = source.path.as_ref() {
        return Ok(MaterializedSource {
            source_index,
            label: path.clone(),
            path: PathBuf::from(path),
            temporary: false,
        });
    }
    if let Some(url) = source.url.as_ref() {
        return fetch_url_to_temp_file(url).map(|path| MaterializedSource {
            source_index,
            label: url.clone(),
            path,
            temporary: true,
        });
    }
    Err("Provide exactly one of path or url for each PDF source.".into())
}

fn read_pdf(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Unable to access file at '{}': {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("Path '{}' is not a regular file.", path.display()));
    }
    if metadata.len() > DEFAULT_MAX_FILE_BYTES {
        return Err(format!(
            "File exceeds maximum size of {DEFAULT_MAX_FILE_BYTES} bytes."
        ));
    }
    fs::read(path).map_err(|error| format!("Failed to read PDF bytes: {error}"))
}

fn render_warnings(
    invalid: &[u32],
    truncated: &[u32],
    num_pages: usize,
    max_pages: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if !invalid.is_empty() {
        warnings.push(format!(
            "Requested pages {} exceed document page count {num_pages}.",
            invalid
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !truncated.is_empty() {
        warnings.push(format!(
            "Rendered first {max_pages} selected pages; skipped {} due to max_pages.",
            truncated
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    warnings
}
