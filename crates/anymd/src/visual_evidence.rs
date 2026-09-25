//! TS-compatible page render and region crop orchestration over the pure-Rust kernel.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use anymd_core::render::{
    crop_pixels_for_bounding_box, crop_rendered_page_png_with_limit, BoundingBox, RenderDocument,
    DEFAULT_MAX_RENDER_OUTPUT_BYTES, DEFAULT_MAX_RENDER_PIXELS, DEFAULT_RENDER_SCALE,
};
use anymd_core::url_fetch::{cleanup_temp_file, fetch_url_to_temp_file};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};

use crate::page_selection::selected_pages;
use crate::schema::{
    PdfEvidenceArgs, PdfEvidenceRegion, PdfEvidenceSource, PdfSource, RegionBoundingBox,
};

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

#[derive(Debug)]
pub(crate) struct RenderedOcrPage {
    pub(crate) page: u32,
    pub(crate) png: Vec<u8>,
    pub(crate) evidence_id: String,
    pub(crate) scale: f64,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug)]
pub(crate) struct RenderedOcrSource {
    pub(crate) source_index: usize,
    pub(crate) source: String,
    pub(crate) pages: Vec<RenderedOcrPage>,
    pub(crate) warnings: Vec<String>,
    pub(crate) error: Option<String>,
}

pub(crate) struct OcrRenderRequest<'a> {
    pub(crate) requested_pages: &'a [u32],
    pub(crate) scale_value: f64,
    pub(crate) max_pages: usize,
    pub(crate) max_pixels: u64,
    pub(crate) request_deadline: std::time::Instant,
    pub(crate) deadline_error: &'a str,
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

pub(crate) fn render_ocr_source(
    source: &MaterializedSource,
    request: OcrRenderRequest<'_>,
    work_budget: &mut RequestWorkBudget,
) -> RenderedOcrSource {
    let output = (|| -> Result<(Vec<RenderedOcrPage>, Vec<String>), String> {
        work_budget.ensure_available()?;
        if std::time::Instant::now() >= request.request_deadline {
            let error = work_budget
                .exhaust(request.deadline_error.to_string())
                .expect_err("deadline exhausts render budget");
            return Err(error);
        }
        let document =
            RenderDocument::new(read_pdf(source.path())?).map_err(|error| error.message)?;
        let num_pages = document.page_count();
        if num_pages == 0 {
            return Err("PDF contains no pages.".into());
        }
        let valid = request
            .requested_pages
            .iter()
            .copied()
            .filter(|page| (*page as usize) <= num_pages)
            .collect::<Vec<_>>();
        let invalid = request
            .requested_pages
            .iter()
            .copied()
            .filter(|page| (*page as usize) > num_pages)
            .collect::<Vec<_>>();
        let pages_to_render = &valid[..valid.len().min(request.max_pages)];
        let truncated = &valid[pages_to_render.len()..];
        if pages_to_render.is_empty() {
            return Err(format!(
                "No valid OCR candidate pages for source {}.",
                source.label()
            ));
        }
        let warnings = render_warnings(&invalid, truncated, num_pages, request.max_pages);
        let scale = request.scale_value as f32;
        let mut pages = Vec::with_capacity(pages_to_render.len());
        for page in pages_to_render {
            work_budget.ensure_available()?;
            if std::time::Instant::now() >= request.request_deadline {
                let error = work_budget
                    .exhaust(request.deadline_error.to_string())
                    .expect_err("deadline exhausts render budget");
                return Err(error);
            }
            let pixels = document
                .planned_render_pixel_count(*page as usize, scale)
                .map_err(|error| error.message)?;
            work_budget.charge_page(pixels)?;
            let rendered = document
                .render_page(
                    *page as usize,
                    scale,
                    request.max_pixels,
                    DEFAULT_MAX_RENDER_OUTPUT_BYTES,
                )
                .map_err(|error| error.message)?;
            pages.push(RenderedOcrPage {
                page: rendered.page as u32,
                png: rendered.png,
                evidence_id: format!(
                    "page-{}-render-scale-{}",
                    rendered.page, request.scale_value
                ),
                scale: request.scale_value,
                width: rendered.width,
                height: rendered.height,
            });
        }
        Ok((pages, warnings))
    })();
    match output {
        Ok((pages, warnings)) => RenderedOcrSource {
            source_index: source.source_index(),
            source: source.label().to_string(),
            pages,
            warnings,
            error: None,
        },
        Err(error) => RenderedOcrSource {
            source_index: source.source_index(),
            source: source.label().to_string(),
            pages: Vec::new(),
            warnings: Vec::new(),
            error: Some(error),
        },
    }
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

pub fn render_pages(value: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let args = parse_args(value)?;
    let scale_value = args.scale.unwrap_or(f64::from(DEFAULT_RENDER_SCALE));
    let scale = scale_value as f32;
    let max_pages = args
        .max_pages
        .map_or(DEFAULT_MAX_RENDER_PAGES, |v| v as usize);
    let max_pixels = args
        .max_pixels_per_page
        .unwrap_or(DEFAULT_MAX_RENDER_PIXELS);
    let include_image = args.include_image.unwrap_or(true);
    let mut results = Vec::with_capacity(args.sources.len());
    let mut images = Vec::new();
    let mut aggregate_bytes = 0usize;
    let mut work_budget = RequestWorkBudget::default();

    for source in &args.sources {
        let source_label = source.label();
        let image_base_index = images.len();
        let output = (|| -> Result<(Value, Vec<ImagePart>, usize), String> {
            work_budget.ensure_available()?;
            let mut source_images = Vec::new();
            let mut source_bytes = 0usize;
            let materialized = materialize(source)?;
            let document = RenderDocument::new(read_pdf(&materialized.path)?)
                .map_err(|error| error.message)?;
            let num_pages = document.page_count();
            if num_pages == 0 {
                return Err("PDF contains no pages.".into());
            }
            let requested = selected_pages(&source.pages)?.unwrap_or_else(|| vec![1]);
            let valid: Vec<u32> = requested
                .iter()
                .copied()
                .filter(|page| (*page as usize) <= num_pages)
                .collect();
            let invalid: Vec<u32> = requested
                .iter()
                .copied()
                .filter(|page| (*page as usize) > num_pages)
                .collect();
            let pages_to_render = &valid[..valid.len().min(max_pages)];
            let truncated = &valid[pages_to_render.len()..];
            if pages_to_render.is_empty() {
                return Err(format!(
                    "No valid pages to render for source {}.",
                    materialized.label
                ));
            }
            let warnings = render_warnings(&invalid, truncated, num_pages, max_pages);
            let mut rendered_pages = Vec::with_capacity(pages_to_render.len());
            for page in pages_to_render {
                let pixels = document
                    .planned_render_pixel_count(*page as usize, scale)
                    .map_err(|error| error.message)?;
                work_budget.charge_page(pixels)?;
                let rendered = document
                    .render_page(
                        *page as usize,
                        scale,
                        max_pixels,
                        DEFAULT_MAX_RENDER_OUTPUT_BYTES,
                    )
                    .map_err(|error| error.message)?;
                source_bytes = source_bytes
                    .checked_add(rendered.png.len())
                    .ok_or_else(|| "Rendered image byte count overflow.".to_string())?;
                if aggregate_bytes
                    .checked_add(source_bytes)
                    .is_none_or(|total| total > MAX_AGGREGATE_IMAGE_BYTES)
                {
                    return Err(format!(
                        "Rendered images exceed aggregate byte limit {MAX_AGGREGATE_IMAGE_BYTES}."
                    ));
                }
                let image_content_index =
                    include_image.then_some(image_base_index + source_images.len() + 1);
                if include_image {
                    source_images.push(ImagePart {
                        data: base64::engine::general_purpose::STANDARD.encode(&rendered.png),
                        mime_type: rendered.mime_type,
                    });
                }
                let mut summary = json!({
                    "page": rendered.page,
                    "evidence_id": format!("page-{}-render-scale-{scale_value}", rendered.page),
                    "width": rendered.width,
                    "height": rendered.height,
                    "scale": scale_value,
                    "pixel_count": rendered.pixel_count,
                    "byte_length": rendered.png.len(),
                    "format": rendered.format,
                    "mime_type": rendered.mime_type,
                    "rotation": rendered.rotation,
                    "provenance": rendered.provenance,
                });
                if let Some(index) = image_content_index {
                    summary["image_content_index"] = json!(index);
                }
                rendered_pages.push(summary);
            }
            let mut result = json!({
                "source": materialized.label,
                "success": true,
                "num_pages": num_pages,
                "rendered_pages": rendered_pages,
            });
            if !warnings.is_empty() {
                result["warnings"] = json!(warnings);
            }
            Ok((result, source_images, source_bytes))
        })();
        match output {
            Ok((result, source_images, source_bytes)) => {
                aggregate_bytes += source_bytes;
                images.extend(source_images);
                results.push(result);
            }
            Err(error) => {
                results.push(json!({ "source": source_label, "success": false, "error": error }))
            }
        }
    }

    if results.iter().all(|result| result["success"] != true) {
        let errors = results
            .iter()
            .filter_map(|result| result["error"].as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return Ok(all_failed(format!(
            "All PDF sources failed to render: {errors}"
        )));
    }
    Ok(tool_result(
        json!({
            "profile": "page_render_evidence",
            "render_options": {
                "scale": scale_value,
                "max_pages": max_pages,
                "max_pixels_per_page": max_pixels,
                "include_image": include_image,
            },
            "results": results,
        }),
        images,
    ))
}

fn region_warnings(
    invalid_pages: &[u32],
    truncated_count: usize,
    total_pages: usize,
    max_regions: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if !invalid_pages.is_empty() {
        let mut pages = invalid_pages.to_vec();
        pages.sort_unstable();
        pages.dedup();
        warnings.push(format!(
            "Requested region pages {} exceed document page count {total_pages}.",
            pages
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if truncated_count > 0 {
        warnings.push(format!(
            "Cropped first {max_regions} valid regions; skipped {truncated_count} due to max_regions."
        ));
    }
    warnings
}

fn bounding_box(value: &RegionBoundingBox) -> BoundingBox {
    BoundingBox {
        left: value.left,
        bottom: value.bottom,
        right: value.right,
        top: value.top,
    }
}

pub fn extract_regions(value: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let args = parse_args(value)?;
    if args
        .sources
        .iter()
        .any(|source| source.regions.as_ref().is_none_or(Vec::is_empty))
    {
        return Ok(all_failed(
            "pdf_evidence operation requires sources[].regions for extract_regions and analyze_regions."
                .into(),
        ));
    }
    let scale_value = args.scale.unwrap_or(f64::from(DEFAULT_RENDER_SCALE));
    let scale = scale_value as f32;
    let max_regions = args.max_regions.map_or(DEFAULT_MAX_REGIONS, |v| v as usize);
    let max_pixels = args
        .max_pixels_per_page
        .unwrap_or(DEFAULT_MAX_RENDER_PIXELS);
    let include_image = args.include_image.unwrap_or(true);
    let mut results = Vec::with_capacity(args.sources.len());
    let mut images = Vec::new();
    let mut aggregate_bytes = 0usize;
    let mut work_budget = RequestWorkBudget::default();

    for source in &args.sources {
        let source_label = source.label();
        let image_base_index = images.len();
        let output = (|| -> Result<(Value, Vec<ImagePart>, usize), String> {
            work_budget.ensure_available()?;
            let mut source_images = Vec::new();
            let mut source_bytes = 0usize;
            let materialized = materialize(source)?;
            let document = RenderDocument::new(read_pdf(&materialized.path)?)
                .map_err(|error| error.message)?;
            let num_pages = document.page_count();
            if num_pages == 0 {
                return Err("PDF contains no pages.".into());
            }
            let requested = source.regions.as_ref().expect("regions checked");
            let invalid_pages: Vec<u32> = requested
                .iter()
                .filter(|region| region.page as usize > num_pages)
                .map(|region| region.page)
                .collect();
            let valid: Vec<(usize, &PdfEvidenceRegion)> = requested
                .iter()
                .enumerate()
                .filter(|(_, region)| region.page as usize <= num_pages)
                .collect();
            let regions_to_crop = &valid[..valid.len().min(max_regions)];
            if regions_to_crop.is_empty() {
                return Err(format!(
                    "No valid regions to crop for source {}.",
                    materialized.label
                ));
            }
            let warnings = region_warnings(
                &invalid_pages,
                valid.len() - regions_to_crop.len(),
                num_pages,
                max_regions,
            );
            let mut by_page: Vec<(u32, Vec<(usize, &PdfEvidenceRegion)>)> = Vec::new();
            for (index, region) in regions_to_crop {
                if let Some((_, regions)) =
                    by_page.iter_mut().find(|(page, _)| *page == region.page)
                {
                    regions.push((*index, *region));
                } else {
                    by_page.push((region.page, vec![(*index, *region)]));
                }
            }
            let mut cropped_regions = Vec::with_capacity(regions_to_crop.len());
            for (page_number, page_regions) in by_page {
                let pixels = document
                    .planned_render_pixel_count(page_number as usize, scale)
                    .map_err(|error| error.message)?;
                work_budget.charge_page(pixels)?;
                let rendered = document
                    .render_page(
                        page_number as usize,
                        scale,
                        max_pixels,
                        DEFAULT_MAX_RENDER_OUTPUT_BYTES,
                    )
                    .map_err(|error| error.message)?;
                for (source_index, region) in page_regions {
                    work_budget.charge_region()?;
                    // Public v3.0.14 compatibility: PDF.js applied the same
                    // bottom-left formula to the rotated viewport dimensions
                    // without transforming the requested PDF coordinates.
                    let pixels = crop_pixels_for_bounding_box(
                        rendered.width,
                        rendered.height,
                        rendered.scale,
                        0,
                        bounding_box(&region.bounding_box),
                        region.padding.unwrap_or(0.0),
                    )
                    .map_err(|error| error.message)?;
                    let png = crop_rendered_page_png_with_limit(
                        &rendered.png,
                        rendered.width,
                        rendered.height,
                        pixels,
                        DEFAULT_MAX_RENDER_OUTPUT_BYTES,
                    )
                    .map_err(|error| error.message)?;
                    source_bytes = source_bytes
                        .checked_add(png.len())
                        .ok_or_else(|| "Cropped image byte count overflow.".to_string())?;
                    if aggregate_bytes
                        .checked_add(source_bytes)
                        .is_none_or(|total| total > MAX_AGGREGATE_IMAGE_BYTES)
                    {
                        return Err(format!(
                            "Cropped images exceed aggregate byte limit {MAX_AGGREGATE_IMAGE_BYTES}."
                        ));
                    }
                    let region_id = region
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("region-{}", source_index + 1));
                    let image_content_index =
                        include_image.then_some(image_base_index + source_images.len() + 1);
                    if include_image {
                        source_images.push(ImagePart {
                            data: base64::engine::general_purpose::STANDARD.encode(&png),
                            mime_type: "image/png",
                        });
                    }
                    let mut summary = json!({
                        "region_id": region_id,
                        "page": page_number,
                        "evidence_id": format!("page-{page_number}-{region_id}-crop-scale-{scale_value}"),
                        "source_bounding_box": region.bounding_box,
                        "crop_pixels": pixels,
                        "scale": scale_value,
                        "byte_length": png.len(),
                        "format": "png",
                        "mime_type": "image/png",
                        "provenance": {
                            "engine": rendered.provenance.engine,
                            "renderer": rendered.provenance.renderer,
                            "source": "region-crop",
                            "page_render_evidence_id": format!("page-{page_number}-render-scale-{scale_value}"),
                        },
                    });
                    if let Some(index) = image_content_index {
                        summary["image_content_index"] = json!(index);
                    }
                    cropped_regions.push(summary);
                }
            }
            let mut result = json!({
                "source": materialized.label,
                "success": true,
                "num_pages": num_pages,
                "regions": cropped_regions,
            });
            if !warnings.is_empty() {
                result["warnings"] = json!(warnings);
            }
            Ok((result, source_images, source_bytes))
        })();
        match output {
            Ok((result, source_images, source_bytes)) => {
                aggregate_bytes += source_bytes;
                images.extend(source_images);
                results.push(result);
            }
            Err(error) => {
                results.push(json!({ "source": source_label, "success": false, "error": error }))
            }
        }
    }

    if results.iter().all(|result| result["success"] != true) {
        let errors = results
            .iter()
            .filter_map(|result| result["error"].as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return Ok(all_failed(format!(
            "All PDF sources failed region extraction: {errors}"
        )));
    }
    Ok(tool_result(
        json!({
            "profile": "region_crop_evidence",
            "crop_options": {
                "scale": scale_value,
                "max_regions": max_regions,
                "max_pixels_per_page": max_pixels,
                "include_image": include_image,
            },
            "results": results,
        }),
        images,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_selection::MAX_SELECTED_PAGES;
    use crate::schema::PageSpecifier;

    fn fixture_path() -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential/v3014-behavior-v1.pdf")
            .canonicalize()
            .expect("canonical fixture")
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn page_selection_matches_ts_sort_dedupe_and_prefix_parse() {
        assert_eq!(
            selected_pages(&Some(PageSpecifier::Range("3,1,1,2x".into()))).unwrap(),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            selected_pages(&Some(PageSpecifier::Range("2-4".into()))).unwrap(),
            Some(vec![2, 3, 4])
        );
        for open in ["1--5", "1-   "] {
            let pages = selected_pages(&Some(PageSpecifier::Range(open.into())))
                .unwrap()
                .unwrap();
            assert_eq!(pages.len(), MAX_SELECTED_PAGES);
            assert_eq!(pages.first(), Some(&1));
            assert_eq!(pages.last(), Some(&10_001));
        }
        assert_eq!(
            selected_pages(&Some(PageSpecifier::Range("1-2-999".into()))).unwrap(),
            Some(vec![1, 2])
        );
    }

    #[test]
    fn warning_text_matches_v3014_contract() {
        assert_eq!(
            render_warnings(&[9], &[3, 4], 2, 2),
            vec![
                "Requested pages 9 exceed document page count 2.",
                "Rendered first 2 selected pages; skipped 3, 4 due to max_pages.",
            ]
        );
        assert_eq!(
            region_warnings(&[9, 9], 2, 3, 1),
            vec![
                "Requested region pages 9 exceed document page count 3.",
                "Cropped first 1 valid regions; skipped 2 due to max_regions.",
            ]
        );
    }

    #[test]
    fn public_render_envelope_has_raw_image_part_and_no_inline_data() {
        let result = render_pages(json!({
            "operation": "render_page",
            "sources": [{"path": fixture_path(), "pages": [1]}],
            "scale": 1,
            "max_pages": 1,
        }))
        .expect("render result");
        let encoded = serde_json::to_value(result).expect("serialize result");
        assert_eq!(encoded["isError"], false);
        assert_eq!(encoded["content"][0]["type"], "text");
        assert_eq!(encoded["content"][1]["type"], "image");
        assert_eq!(encoded["content"][1]["mimeType"], "image/png");
        let image = encoded["content"][1]["data"].as_str().expect("image data");
        assert!(!image.starts_with("data:"));
        let payload: Value = serde_json::from_str(
            encoded["content"][0]["text"]
                .as_str()
                .expect("text payload"),
        )
        .expect("parse text payload");
        assert_eq!(payload["profile"], "page_render_evidence");
        assert_eq!(payload["results"][0]["rendered_pages"][0]["page"], 1);
        assert_eq!(
            payload["results"][0]["rendered_pages"][0]["image_content_index"],
            1
        );
        assert!(payload["results"][0]["rendered_pages"][0]
            .get("data")
            .is_none());
    }

    #[test]
    fn render_selection_and_region_group_order_match_v3014() {
        let render = render_pages(json!({
            "operation": "render_page",
            "sources": [{"path": fixture_path(), "pages": [2, 99, 1, 2]}],
            "scale": 1,
            "max_pages": 1,
            "include_image": false,
        }))
        .expect("render result");
        let payload = render.structured_content.expect("structured render");
        assert_eq!(payload["results"][0]["rendered_pages"][0]["page"], 1);
        assert_eq!(
            payload["results"][0]["warnings"],
            json!([
                "Requested pages 99 exceed document page count 3.",
                "Rendered first 1 selected pages; skipped 2 due to max_pages."
            ])
        );
        assert_eq!(render.content.len(), 1);
        assert!(payload["results"][0]["rendered_pages"][0]
            .get("image_content_index")
            .is_none());

        let crops = extract_regions(json!({
            "operation": "extract_regions",
            "sources": [{
                "path": fixture_path(),
                "regions": [
                    {"id": "A", "page": 2, "bounding_box": {"left": 0, "bottom": 0, "right": 20, "top": 20}},
                    {"page": 1, "bounding_box": {"left": 0, "bottom": 0, "right": 20, "top": 20}},
                    {"id": "C", "page": 2, "bounding_box": {"left": 20, "bottom": 20, "right": 40, "top": 40}}
                ]
            }],
            "scale": 1,
            "include_image": false,
        }))
        .expect("crop result");
        let payload = crops.structured_content.expect("structured crops");
        let ids = payload["results"][0]["regions"]
            .as_array()
            .expect("regions")
            .iter()
            .map(|region| region["region_id"].as_str().expect("region id"))
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["A", "C", "region-2"]);
        assert_eq!(crops.content.len(), 1);
    }

    #[test]
    fn late_source_failure_discards_staged_images_and_preserves_global_indexes() {
        let path = fixture_path();
        let crops = extract_regions(json!({
            "operation": "extract_regions",
            "sources": [
                {
                    "path": path,
                    "regions": [
                        {"id": "staged", "page": 1, "bounding_box": {"left": 0, "bottom": 0, "right": 20, "top": 20}},
                        {"id": "late-failure", "page": 1, "bounding_box": {"left": 10, "bottom": 10, "right": 10, "top": 20}}
                    ]
                },
                {
                    "path": fixture_path(),
                    "regions": [
                        {"id": "survivor", "page": 1, "bounding_box": {"left": 0, "bottom": 0, "right": 20, "top": 20}}
                    ]
                }
            ],
            "scale": 1,
        }))
        .expect("mixed crop result");
        let payload = crops.structured_content.expect("structured crops");
        assert_eq!(payload["results"][0]["success"], false);
        assert_eq!(payload["results"][1]["success"], true);
        assert_eq!(
            payload["results"][1]["regions"][0]["image_content_index"],
            1
        );
        assert_eq!(crops.content.len(), 2);
    }

    #[test]
    fn request_work_budget_is_charged_before_work_and_never_rolls_back() {
        let mut budget = RequestWorkBudget::default();
        for _ in 0..MAX_REQUEST_RENDERED_PAGES {
            budget.charge_page(1).expect("page within bound");
        }
        assert!(budget.charge_page(1).unwrap_err().contains("pages"));
        assert_eq!(budget.rendered_pages, MAX_REQUEST_RENDERED_PAGES);
        assert!(budget
            .ensure_available()
            .expect_err("page exhaustion is sticky")
            .contains("pages"));

        let mut pixel_budget = RequestWorkBudget::default();
        pixel_budget
            .charge_page(MAX_REQUEST_RENDER_PIXELS)
            .expect("pixels at bound");
        assert!(pixel_budget.charge_page(1).unwrap_err().contains("pixels"));
        assert_eq!(pixel_budget.render_pixels, MAX_REQUEST_RENDER_PIXELS);
        assert!(pixel_budget
            .ensure_available()
            .expect_err("pixel exhaustion is sticky")
            .contains("pixels"));

        let mut region_budget = RequestWorkBudget::default();
        for _ in 0..MAX_REQUEST_REGIONS {
            region_budget.charge_region().expect("region within bound");
        }
        assert!(region_budget
            .charge_region()
            .unwrap_err()
            .contains("regions"));
        assert_eq!(region_budget.regions, MAX_REQUEST_REGIONS);
        assert!(region_budget
            .ensure_available()
            .expect_err("region exhaustion is sticky")
            .contains("regions"));
    }

    #[test]
    fn source_count_is_rejected_before_any_io_or_render_work() {
        let sources = (0..=MAX_SOURCES_PER_REQUEST)
            .map(|index| json!({"path": format!("missing-{index}.pdf")}))
            .collect::<Vec<_>>();
        let error = render_pages(json!({
            "operation": "render_page",
            "sources": sources,
        }))
        .expect_err("too many sources");
        assert!(error.message.contains("at most 32 sources"));
    }
}
