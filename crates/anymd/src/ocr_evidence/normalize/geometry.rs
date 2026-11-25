use serde_json::{json, Value};

// Existing provider normalization emits bottom-left boxes divided by scale.
// Recover raster corners and use the actual render inverse, not scale alone.
pub(in crate::ocr_evidence) fn restore_render_geometry(
    output: &mut Value,
    transform: [f64; 6],
    scale: f64,
    height: u32,
) {
    if let Some(words) = output.get_mut("words").and_then(Value::as_array_mut) {
        for word in words {
            let mapped = word
                .get("bounding_box")
                .cloned()
                .and_then(|value| {
                    serde_json::from_value::<anymd_core::render::BoundingBox>(value).ok()
                })
                .and_then(|b| {
                    anymd_core::render::pdf_box_from_pixel_box(
                        transform,
                        anymd_core::render::BoundingBox {
                            left: b.left * scale,
                            right: b.right * scale,
                            bottom: f64::from(height) - b.top * scale,
                            top: f64::from(height) - b.bottom * scale,
                        },
                    )
                    .ok()
                });
            if let Some(b) = mapped {
                word["bounding_box"] = json!(b);
            } else if let Some(object) = word.as_object_mut() {
                object.remove("bounding_box");
            }
        }
    }
}

/// Convert normalized bottom-left boxes into video frame coordinates.
pub(in crate::ocr_evidence) fn raster_observation(
    value: &Value,
    height: f64,
    builtin_vlm: bool,
) -> Result<crate::video_evidence::RasterObservation, String> {
    let mut regions = Vec::new();
    for word in value["words"].as_array().into_iter().flatten() {
        let bbox = &word["bounding_box"];
        let (Some(left), Some(right), Some(top), Some(bottom)) = (
            bbox["left"].as_f64(),
            bbox["right"].as_f64(),
            bbox["top"].as_f64(),
            bbox["bottom"].as_f64(),
        ) else {
            continue;
        };
        regions.push(crate::video_evidence::RasterRegion {
            text: word["text"].as_str().unwrap_or("").into(),
            left,
            right,
            top: height - top,
            bottom: height - bottom,
            geometry_level: if builtin_vlm {
                "ocr_region"
            } else {
                "ocr_word"
            }
            .into(),
        });
    }
    if regions.is_empty() && !value["text"].as_str().unwrap_or("").is_empty() {
        return Err("OCR text lacks frame geometry".into());
    }
    Ok(crate::video_evidence::RasterObservation {
        provider: if builtin_vlm { "doc-vlm" } else { "command" }.into(),
        model_revision: value["provenance"]["model_revision"]
            .as_str()
            .map(str::to_string),
        coordinate_space: "frame_pixels_top_left".into(),
        regions,
        truncated: value["truncated"].as_bool().unwrap_or(false),
    })
}
