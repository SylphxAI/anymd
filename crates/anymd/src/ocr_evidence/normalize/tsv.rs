use serde_json::{json, Value};

use crate::pdf_evidence_common::truncate_utf16;

use super::words::{normalize_confidence, normalize_words};

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

pub(super) fn parse_tesseract_tsv_output(
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
        return raw_tsv_fallback(stdout, max_output_chars, languages);
    };
    let Some(image_height) = image_height.filter(|height| *height > 0.0) else {
        return raw_tsv_fallback(stdout, max_output_chars, languages);
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

fn raw_tsv_fallback(stdout: &str, max_output_chars: usize, languages: &[String]) -> Value {
    let (text, truncated) = truncate_utf16(stdout.trim(), max_output_chars);
    let mut warnings =
        vec!["Tesseract TSV output could not be normalized; returned raw OCR output.".to_string()];
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
    output
}
