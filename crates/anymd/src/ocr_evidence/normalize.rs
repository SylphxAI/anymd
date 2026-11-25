//! Normalize OCR provider output and map word boxes back into PDF space.

mod geometry;
mod provider_json;
mod tsv;
mod words;

use serde_json::Value;

use super::provider::OcrOutputFormat;

pub(super) use geometry::{raster_observation, restore_render_geometry};

pub(super) fn normalize_output(
    stdout: &str,
    max_output_chars: usize,
    languages: &[String],
    scale: f64,
    output_format: OcrOutputFormat,
    image_height: Option<f64>,
) -> Value {
    if output_format == OcrOutputFormat::TesseractTsv {
        return tsv::parse_tesseract_tsv_output(
            stdout,
            max_output_chars,
            languages,
            image_height,
            scale,
        );
    }
    provider_json::normalize_json_or_text(stdout, max_output_chars, languages, scale)
}
