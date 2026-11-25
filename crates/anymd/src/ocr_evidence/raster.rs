use std::time::Instant;

use serde_json::json;

use super::normalize::{normalize_output, raster_observation};
use super::provider::{provider_config, run_provider};
use super::OcrRequestPermit;

/// Video raster input borrows the same OCR request permit; no PDF-space fiction.
pub(crate) fn recognize_raster(
    frame: &anymd_formats::video::timeline::DecodedFrame,
    engine: crate::ocr_vlm::OcrEngine,
    deadline: Instant,
    permit: &OcrRequestPermit,
) -> Result<crate::video_evidence::RasterObservation, String> {
    let config = provider_config(Some(engine))?;
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64;
    if remaining == 0 {
        return Err("video request deadline exceeded".into());
    }
    let mut value = if config.builtin_vlm() {
        let page = crate::ocr_vlm::recognize_admitted(&frame.png, deadline, permit)?;
        let mut value = serde_json::to_value(&page).map_err(|e| e.to_string())?;
        value["words"] = json!(page.regions.iter().map(|r| json!({"text":r.text,"region_type":r.label,"bounding_box":{"left":r.bbox[0],"right":r.bbox[2],"top":frame.metadata.height as f32-r.bbox[1],"bottom":frame.metadata.height as f32-r.bbox[3]}})).collect::<Vec<_>>());
        value["model_revision"] = json!(crate::ocr_vlm::model_revision());
        value
    } else {
        let stdout = run_provider(
            &config,
            &frame.png,
            0,
            &frame.metadata.sha256,
            &[],
            remaining.min(60_000),
            200_000,
        )
        .map_err(|e| e.message)?;
        normalize_output(
            &stdout,
            200_000,
            &[],
            1.0,
            config.output_format,
            Some(frame.metadata.height as f64),
        )
    };
    config.stamp_provenance(&mut value);
    raster_observation(
        &value,
        f64::from(frame.metadata.height),
        config.builtin_vlm(),
    )
}
