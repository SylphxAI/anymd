use anymd_core::render::{
    crop_pixels_for_bounding_box, crop_rendered_page_png_with_limit, BoundingBox, RenderDocument,
    DEFAULT_MAX_RENDER_OUTPUT_BYTES, DEFAULT_MAX_RENDER_PIXELS, DEFAULT_RENDER_SCALE,
};
use base64::Engine;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::schema::{PdfEvidenceRegion, RegionBoundingBox};

use super::{
    all_failed, materialize, parse_args, read_pdf, tool_result, ImagePart, RequestWorkBudget,
    DEFAULT_MAX_REGIONS, MAX_AGGREGATE_IMAGE_BYTES,
};

pub(super) fn region_warnings(
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
