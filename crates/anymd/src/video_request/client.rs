use super::cache::{hash, materialize, remaining};
use super::sidecars::sidecar_text;
use super::worker::Work;
use crate::source_access::SourceAccessPolicy;
use crate::video_evidence::VideoEvidence;
use anymd_formats::video::timeline::{self, TimelineOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineSelection {
    #[serde(default)]
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default = "twenty")]
    pub max_scenes: usize,
    #[serde(default)]
    pub caption: bool,
}
fn twenty() -> usize {
    20
}
impl TimelineSelection {
    pub(super) fn options(&self) -> TimelineOptions {
        TimelineOptions {
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            max_scenes: self.max_scenes,
            caption: self.caption,
        }
    }
}

pub(super) struct RequestOptions<'a> {
    pub(super) selection: Option<TimelineSelection>,
    pub(super) timestamps: Option<Vec<u64>>,
    pub(super) transcript: bool,
    pub(super) ocr: Option<crate::ocr_vlm::OcrEngine>,
    pub(super) expected: Option<&'a str>,
    pub(super) timeout: u64,
}

pub(super) fn request(
    source: &str,
    options: RequestOptions<'_>,
    policy: &SourceAccessPolicy,
) -> Result<Value, String> {
    let RequestOptions {
        selection,
        timestamps,
        transcript,
        ocr,
        expected,
        timeout,
    } = options;
    if !(1000..=300_000).contains(&timeout) {
        return Err("timeout_ms must be 1000..300000".into());
    }
    if let Some(ref options) = selection {
        options.options().validate()?;
    }
    if let Some(ref positions) = timestamps {
        if positions.is_empty()
            || positions.len() > 20
            || positions
                .iter()
                .any(|v| *v > i64::MAX as u64 / 1000 - timeline::MAX_WINDOW_MS)
        {
            return Err("timestamps_ms must contain 1..20 positions".into());
        }
    }
    let deadline = Instant::now() + Duration::from_millis(timeout);
    let file = materialize(source, policy, deadline)?;
    let source_hash = hash(&std::fs::read(file.path()).map_err(|e| e.to_string())?);
    if expected.is_some_and(|v| v != source_hash) {
        return Err("source SHA-256 mismatch".into());
    }
    let mut sidecars = vec![];
    if !crate::document::is_url(source) && timestamps.is_none() {
        for extension in ["srt", "vtt"] {
            let text = sidecar_text(Path::new(source), extension, policy);
            sidecars.push((extension.into(), text));
        }
    }
    let work = Work {
        path: file.path().into(),
        source_hash,
        sidecars,
        selection,
        timestamps,
        transcript,
        ocr,
        timeout_ms: remaining(deadline)?,
    };
    let mut input = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut input, &work).map_err(|e| e.to_string())?;
    let _permit = if ocr.is_some() {
        Some(crate::ocr_evidence::acquire_ocr_permit()?)
    } else {
        None
    };
    let output =
        crate::command_provider::run_supervised(crate::command_provider::CommandInvocation {
            command: std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned(),
            args: vec![
                "__video-worker".into(),
                input.path().to_string_lossy().into_owned(),
            ],
            timeout_ms: remaining(deadline)?,
            max_stdout_bytes: 4 * 1024 * 1024,
            failure_message: "video worker failed".into(),
            timeout_message: "video request deadline exceeded".into(),
        })
        .map_err(|e| e.message)?;
    serde_json::from_str(&output).map_err(|e| e.to_string())
}
pub(crate) fn document(
    source: &str,
    selection: &TimelineSelection,
    transcript: bool,
    ocr: Option<crate::ocr_vlm::OcrEngine>,
    policy: &SourceAccessPolicy,
) -> Result<VideoEvidence, String> {
    serde_json::from_value(request(
        source,
        RequestOptions {
            selection: Some(selection.clone()),
            timestamps: None,
            transcript,
            ocr,
            expected: None,
            timeout: 60_000,
        },
        policy,
    )?)
    .map_err(|e| e.to_string())
}
pub(crate) fn manifest_id(evidence: &VideoEvidence) -> String {
    hash(
        serde_json::to_string(evidence)
            .unwrap_or_default()
            .as_bytes(),
    )
}
