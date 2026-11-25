//! Local-command raster captioning for video frames.

use serde_json::{json, Value};

use super::normalize::normalize_output;
use super::provider::run_provider;
use super::provider_config::{provider_config, ProviderConfig};

/// Raster caption seam: only an explicitly configured local command is eligible.
pub(crate) fn local_caption_identity() -> Option<String> {
    let ProviderConfig::Command {
        command,
        args_template,
    } = provider_config().ok()?
    else {
        return None;
    };
    Some(format!("{command}:{args_template:?}"))
}

pub(crate) fn caption_raster(
    png: &[u8],
    source: &str,
    width: u32,
    height: u32,
    timeout_ms: u64,
) -> Result<crate::video_evidence::FrameDescription, String> {
    let config = provider_config()?;
    if !matches!(config, ProviderConfig::Command { .. }) {
        return Err("local-command caption provider required".into());
    }
    let region = json!({"region_id":"video-frame", "evidence_id":source, "source_bounding_box":{"left":0,"bottom":0,"right":width,"top":height}});
    let stdout = run_provider(&config, png, source, &region, &[], timeout_ms, 16_000)
        .map_err(|e| e.message)?;
    let value = normalize_output(&stdout, 16_000);
    Ok(crate::video_evidence::FrameDescription {
        text: value["description"].as_str().unwrap_or("").into(),
        provider: "external-command".into(),
        model_revision: serde_json::from_str::<Value>(&stdout)
            .ok()
            .and_then(|v| v["model_revision"].as_str().map(str::to_string)),
        truncated: value["truncated"].as_bool().unwrap_or(false)
            || value["warnings"].as_array().is_some_and(|warnings| {
                warnings
                    .iter()
                    .any(|v| v.as_str().is_some_and(|s| s.contains("truncated")))
            })
            || serde_json::from_str::<Value>(&stdout)
                .ok()
                .is_some_and(|v| v["truncated"].as_bool().unwrap_or(false)),
    })
}
