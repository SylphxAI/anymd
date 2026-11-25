//! Normalize OCR provider output and map word boxes back into PDF space.

use serde_json::{json, Value};

use crate::pdf_evidence_common::truncate_utf16;

use super::provider::OcrOutputFormat;

fn normalize_confidence(value: &Value) -> Option<f64> {
    let value = value.as_f64()?;
    value.is_finite().then(|| {
        let value = if value > 1.0 { value / 100.0 } else { value };
        value.clamp(0.0, 1.0)
    })
}

fn round_coordinate(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn normalize_words(value: &Value, scale: f64) -> Option<Vec<Value>> {
    let words = value
        .as_array()?
        .iter()
        .filter_map(|word| {
            let word = word.as_object()?;
            let text = word.get("text")?.as_str()?;
            if text.trim().is_empty() {
                return None;
            }
            let mut output = json!({"text": text});
            for key in ["reading_order", "region_type", "layout_confidence"] {
                if let Some(value) = word.get(key) {
                    output[key] = value.clone();
                }
            }
            if let Some(confidence) = word.get("confidence").and_then(normalize_confidence) {
                output["confidence"] = json!(confidence);
            }
            if let Some(box_) = word.get("bounding_box").and_then(Value::as_object) {
                let left = box_.get("left").and_then(Value::as_f64);
                let bottom = box_.get("bottom").and_then(Value::as_f64);
                let right = box_.get("right").and_then(Value::as_f64);
                let top = box_.get("top").and_then(Value::as_f64);
                if let (Some(left), Some(bottom), Some(right), Some(top)) =
                    (left, bottom, right, top)
                {
                    if [left, bottom, right, top]
                        .iter()
                        .all(|value| value.is_finite())
                        && right > left
                        && top > bottom
                    {
                        output["bounding_box"] = json!({
                            "left": round_coordinate(left / scale),
                            "bottom": round_coordinate(bottom / scale),
                            "right": round_coordinate(right / scale),
                            "top": round_coordinate(top / scale),
                        });
                    }
                }
            }
            Some(output)
        })
        .collect::<Vec<_>>();
    (!words.is_empty()).then_some(words)
}

fn parse_finite_number(value: Option<&str>) -> Option<f64> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let parsed = value.parse::<f64>().ok()?;
    parsed.is_finite().then_some(parsed)
}

fn required_tsv_column_indexes(headers: &[&str]) -> Option<[usize; 10]> {
    let index = |name: &str| headers.iter().position(|header| *header == name);
    let columns = [
        index("level")?,
        index("block_num")?,
        index("par_num")?,
        index("line_num")?,
        index("left")?,
        index("top")?,
        index("width")?,
        index("height")?,
        index("conf")?,
        index("text")?,
    ];
    Some(columns)
}

fn parse_tesseract_tsv_output(
    stdout: &str,
    max_output_chars: usize,
    languages: &[String],
    image_height: Option<f64>,
    scale: f64,
) -> Value {
    let lines: Vec<&str> = stdout.trim().lines().collect();
    let headers = lines
        .first()
        .map(|line| line.split('\t').collect::<Vec<_>>());
    let columns = headers
        .as_ref()
        .and_then(|headers| required_tsv_column_indexes(headers));
    let Some(columns) = columns else {
        let (text, truncated) = truncate_utf16(stdout.trim(), max_output_chars);
        let mut warnings = vec![
            "Tesseract TSV output could not be normalized; returned raw OCR output.".to_string(),
        ];
        if truncated {
            warnings.insert(
                0,
                format!("OCR output truncated to {max_output_chars} characters."),
            );
        }
        let mut output = json!({"text": text, "warnings": warnings});
        if let Some(language) = languages.first() {
            output["language"] = json!(language);
        }
        return output;
    };
    let Some(image_height) = image_height.filter(|height| *height > 0.0) else {
        let (text, truncated) = truncate_utf16(stdout.trim(), max_output_chars);
        let mut warnings = vec![
            "Tesseract TSV output could not be normalized; returned raw OCR output.".to_string(),
        ];
        if truncated {
            warnings.insert(
                0,
                format!("OCR output truncated to {max_output_chars} characters."),
            );
        }
        let mut output = json!({"text": text, "warnings": warnings});
        if let Some(language) = languages.first() {
            output["language"] = json!(language);
        }
        return output;
    };

    let [level_idx, block_idx, par_idx, line_idx, left_idx, top_idx, width_idx, height_idx, conf_idx, text_idx] =
        columns;

    let mut words = Vec::new();
    let mut line_texts: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for raw_line in lines.iter().skip(1) {
        if raw_line.trim().is_empty() {
            continue;
        }
        let values: Vec<&str> = raw_line.split('\t').collect();
        let level = parse_finite_number(values.get(level_idx).copied());
        let text = values
            .get(text_idx..)
            .map(|parts| parts.join("	").trim().to_string())
            .unwrap_or_default();
        if level != Some(5.0) || text.is_empty() {
            continue;
        }
        let left = parse_finite_number(values.get(left_idx).copied());
        let top = parse_finite_number(values.get(top_idx).copied());
        let width = parse_finite_number(values.get(width_idx).copied());
        let height = parse_finite_number(values.get(height_idx).copied());
        let confidence = parse_finite_number(values.get(conf_idx).copied())
            .and_then(|value| normalize_confidence(&json!(value)));
        let line_key = format!(
            "{}:{}:{}",
            values.get(block_idx).copied().unwrap_or("0"),
            values.get(par_idx).copied().unwrap_or("0"),
            values.get(line_idx).copied().unwrap_or("0")
        );
        line_texts.entry(line_key).or_default().push(text.clone());

        let mut word = json!({"text": text});
        if let Some(confidence) = confidence {
            word["confidence"] = json!(confidence);
        }
        if let (Some(left), Some(top), Some(width), Some(height)) = (left, top, width, height) {
            if width > 0.0 && height > 0.0 {
                // image-space bottom-left box; convert to PDF coords via scale below
                let image_box = json!({
                    "left": left,
                    "bottom": image_height - top - height,
                    "right": left + width,
                    "top": image_height - top,
                });
                word["bounding_box"] = image_box;
            }
        }
        words.push(word);
    }

    let raw_text = line_texts
        .values()
        .map(|line| line.join(" "))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let (text, truncated) = truncate_utf16(&raw_text, max_output_chars);
    let mut output = json!({"text": text});
    let confidences = words
        .iter()
        .filter_map(|word| word.get("confidence").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    if !confidences.is_empty() {
        let average = confidences.iter().sum::<f64>() / confidences.len() as f64;
        output["confidence"] = json!((average * 100.0).round() / 100.0);
    }
    if !words.is_empty() {
        // wrap through normalize_words to apply scale conversion
        let words_value = Value::Array(words);
        if let Some(normalized) = normalize_words(&words_value, scale) {
            output["words"] = json!(normalized);
        }
    }
    if let Some(language) = languages.first() {
        output["language"] = json!(language);
    }
    if truncated {
        output["warnings"] = json!([format!(
            "OCR output truncated to {max_output_chars} characters."
        )]);
    }
    output
}

pub(super) fn normalize_output(
    stdout: &str,
    max_output_chars: usize,
    languages: &[String],
    scale: f64,
    output_format: OcrOutputFormat,
    image_height: Option<f64>,
) -> Value {
    if output_format == OcrOutputFormat::TesseractTsv {
        return parse_tesseract_tsv_output(
            stdout,
            max_output_chars,
            languages,
            image_height,
            scale,
        );
    }
    let trimmed = stdout.trim();
    let parsed = serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object);
    let raw_text = parsed
        .as_ref()
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
        .unwrap_or(trimmed);
    let (text, truncated) = truncate_utf16(raw_text, max_output_chars);
    let mut output = json!({"text": text});
    let mut provenance = serde_json::Map::new();
    for key in ["model_revision", "device", "quantization"] {
        if let Some(value) = parsed
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
        {
            provenance.insert(key.into(), json!(value));
        }
    }
    if !provenance.is_empty() {
        output["provenance"] = json!(provenance);
    }
    if let Some(confidence) = parsed
        .as_ref()
        .and_then(|value| value.get("confidence"))
        .and_then(normalize_confidence)
    {
        output["confidence"] = json!(confidence);
    }
    if let Some(words) = parsed
        .as_ref()
        .and_then(|value| value.get("words"))
        .and_then(|value| normalize_words(value, scale))
    {
        output["words"] = json!(words);
    }
    if let Some(language) = parsed
        .as_ref()
        .and_then(|value| value.get("language"))
        .and_then(Value::as_str)
        .or_else(|| languages.first().map(String::as_str))
    {
        output["language"] = json!(language);
    }
    let mut warnings = Vec::new();
    if truncated {
        warnings.push(format!(
            "OCR output truncated to {max_output_chars} characters."
        ));
    }
    if let Some(stops) = parsed
        .as_ref()
        .and_then(|value| value.get("truncated"))
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
    {
        warnings.push(format!(
            "Doc-VLM generation stopped in {stops} regions (token limit or repetition)."
        ));
    }
    if !warnings.is_empty() {
        output["warnings"] = json!(warnings);
    }
    output
}

// Existing provider normalization emits bottom-left boxes divided by scale.
// Recover raster corners and use the actual render inverse, not scale alone.
pub(super) fn restore_render_geometry(
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
pub(super) fn raster_observation(
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
