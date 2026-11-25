use anymd_core::render::{
    RenderDocument, DEFAULT_MAX_RENDER_OUTPUT_BYTES, DEFAULT_MAX_RENDER_PIXELS,
    DEFAULT_RENDER_SCALE,
};
use base64::Engine;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::page_selection::selected_pages;

use super::{
    all_failed, materialize, parse_args, read_pdf, render_warnings, tool_result, ImagePart,
    RequestWorkBudget, DEFAULT_MAX_RENDER_PAGES, MAX_AGGREGATE_IMAGE_BYTES,
};

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
