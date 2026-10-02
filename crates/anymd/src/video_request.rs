//! Public video integration: admission precedes cache/worker work. Reuses the
//! existing supervised self-worker, OCR permit, local caption adapter and image store.
use crate::schema::{InspectArgs, InspectOperation};
use crate::source_access::SourceAccessPolicy;
use crate::video_evidence::{
    self, FrameContext, FrameDescription, RasterObservation, VideoEvidence,
};
use anymd_formats::video::timeline::{self, DecodedFrame, TimelineOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
    fn options(&self) -> TimelineOptions {
        TimelineOptions {
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            max_scenes: self.max_scenes,
            caption: self.caption,
        }
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn remaining(deadline: Instant) -> Result<u64, String> {
    let ms = deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64;
    if ms == 0 {
        Err("video request deadline exceeded".into())
    } else {
        Ok(ms)
    }
}
/// Snapshot the admitted representation so hashes and extraction cannot diverge.
fn materialize(
    source: &str,
    policy: &SourceAccessPolicy,
    deadline: Instant,
) -> Result<tempfile::NamedTempFile, String> {
    remaining(deadline)?;
    let bytes = if crate::document::is_url(source) {
        anymd_core::url_fetch::fetch_url_deadline(source, deadline)?.bytes
    } else {
        let admitted = policy.admit_path(source)?;
        let mut file = std::fs::File::open(&admitted).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(256 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        bytes
    };
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("video representation exceeds 256 MiB".into());
    }
    remaining(deadline)?;
    let mut file = tempfile::Builder::new()
        .suffix(".media")
        .tempfile()
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(file)
}
#[derive(Serialize, Deserialize)]
struct Work {
    path: PathBuf,
    source_hash: String,
    /// Each sidecar load outcome; a load error is a subtitles gap, not a failed request.
    sidecars: Vec<(String, Result<String, String>)>,
    selection: Option<TimelineSelection>,
    timestamps: Option<Vec<u64>>,
    transcript: bool,
    ocr: Option<crate::ocr_vlm::OcrEngine>,
    timeout_ms: u64,
}
fn artifact(key: &str) -> Result<PathBuf, String> {
    anymd_formats::cache::cache_dir()
        .map(|p| p.join("images").join(format!("video-{key}.json")))
        .ok_or("video cache unavailable".into())
}
fn cache_put<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    let path = artifact(key)?;
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn cache_get<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    let path = artifact(key).ok()?;
    let bytes = std::fs::read(&path).ok()?;
    let result = serde_json::from_slice(&bytes).ok()?;
    anymd_formats::cache::touch(&path);
    Some(result)
}
struct Context {
    deadline: Instant,
    budget: crate::visual_evidence::RequestWorkBudget,
    ocr: Option<crate::ocr_vlm::OcrEngine>,
    request: Option<crate::ocr_vlm::OcrRequest>,
    identity: String,
}
impl FrameContext for Context {
    fn ensure_available(&self, deadline: Instant) -> Result<(), String> {
        self.budget.ensure_available()?;
        remaining(deadline.min(self.deadline)).map(|_| ())
    }
    fn charge_frame(&mut self, pixels: u64, encoded_bytes: u64) -> Result<(), String> {
        self.budget.charge_frame(pixels, encoded_bytes)
    }

    fn ocr(
        &mut self,
        frame: &DecodedFrame,
        deadline: Instant,
    ) -> Result<Option<RasterObservation>, String> {
        let Some(engine) = self.ocr else {
            return Ok(None);
        };
        if self.request.is_none() {
            self.request = Some(crate::ocr_vlm::OcrRequest::acquire()?)
        };
        self.request
            .as_mut()
            .unwrap()
            .run_page(|page_deadline, permit| {
                crate::ocr_evidence::recognize_raster(
                    frame,
                    engine,
                    deadline.min(page_deadline),
                    permit,
                )
            })
            .map(Some)
    }
    fn local_caption_available(&self) -> bool {
        crate::region_analysis_evidence::local_caption_identity().is_some()
    }
    fn caption_cached(
        &mut self,
        source_hash: &str,
        scene_id: &str,
        frame: &DecodedFrame,
        deadline: Instant,
    ) -> Result<Option<FrameDescription>, String> {
        let Some(provider) = crate::region_analysis_evidence::local_caption_identity() else {
            return Ok(None);
        };
        let key = hash(
            format!(
                "caption:{}:{source_hash}:{scene_id}:{}:{provider}",
                self.identity, frame.metadata.sha256
            )
            .as_bytes(),
        );
        if let Some(cached) = cache_get(&key) {
            return Ok(Some(cached));
        }
        let description = crate::region_analysis_evidence::caption_raster(
            &frame.png,
            source_hash,
            frame.metadata.width,
            frame.metadata.height,
            remaining(deadline)?,
        )?;
        if !description.truncated && !description.text.trim().is_empty() {
            cache_put(&key, &description)?;
        }
        Ok(Some(description))
    }
}

fn execute(work: Work) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_millis(work.timeout_ms);
    let identity = hash(
        serde_json::to_string(&(
            &work.source_hash,
            &work.sidecars,
            &work.selection,
            work.transcript,
            work.ocr,
            timeline::POLICY,
            crate::ocr_vlm::model_revision(),
            [
                "ANYMD_OCR_QUANTIZATION",
                "MCP_PDF_OCR_COMMAND",
                "MCP_PDF_OCR_ARGS_JSON",
            ]
            .map(|name| std::env::var(name).ok()),
            crate::region_analysis_evidence::local_caption_identity(),
        ))
        .map_err(|e| e.to_string())?
        .as_bytes(),
    );
    let mut context = Context {
        deadline,
        budget: crate::visual_evidence::RequestWorkBudget::default(),
        ocr: work.ocr,
        request: None,
        identity: identity.clone(),
    };
    if let Some(timestamps) = work.timestamps {
        let Some(clock) = timeline::probe_clock(&work.path, deadline)? else {
            return Err("video clock unavailable".into());
        };
        let frames =
            video_evidence::render_frames(&work.path, &clock, &timestamps, deadline, &mut context)?;
        let store = anymd_formats::images::ImageStore::default_location()
            .ok_or("image cache unavailable")?;
        let mut results = vec![];
        for (requested, frame) in timestamps.iter().zip(frames) {
            results.push(match frame {Some(frame)=>json!({"requested_ms":requested,"frame":frame.metadata,"image_path":store.put(&frame.png)?.path}),None=>json!({"requested_ms":requested,"status":"unavailable"})});
        }
        return Ok(json!({"source_sha256":work.source_hash,"frames":results}));
    }
    if let Some(cached) = cache_get::<VideoEvidence>(&identity) {
        return serde_json::to_value(cached).map_err(|e| e.to_string());
    }
    let options = work.selection.ok_or("timeline required")?.options();
    let mut timeline = timeline::extract(&work.path, &work.source_hash, &options, deadline)?;
    attach_sidecars(&mut timeline, &work.sidecars);
    if work.transcript {
        if let Some(clock) = timeline.clock.as_ref() {
            match anymd_formats::video::transcript_window(&work.path, clock, &options, deadline) {
                Ok(cues) => apply_transcript_cues(&mut timeline, cues),
                Err(message) => timeline.components.transcript = timeline::Component::gap(message),
            }
        } else {
            timeline.components.transcript = timeline::Component::gap("media clock unavailable");
        }
    }
    let evidence = video_evidence::enrich(
        &work.path,
        timeline,
        &options,
        work.ocr.is_some(),
        deadline,
        &mut context,
    )?;
    if cacheable(&evidence) {
        cache_put(&identity, &evidence)?;
    }
    serde_json::to_value(evidence).map_err(|e| e.to_string())
}
fn apply_transcript_cues(timeline: &mut timeline::Timeline, cues: Vec<timeline::Cue>) {
    if let Err(reason) = timeline::attach_cues(timeline, cues, true) {
        timeline.components.transcript =
            timeline::Component::gap(format!("transcript cues rejected: {reason}"));
    }
}

/// A subtitle file error (load or parse) is a gap on that component, never a
/// failed request. Reasons accumulate and the worse status is kept.
fn attach_sidecars(
    timeline: &mut timeline::Timeline,
    sidecars: &[(String, Result<String, String>)],
) {
    for (name, loaded) in sidecars {
        let attached = loaded.as_ref().map_err(String::clone).and_then(|text| {
            let cues = timeline::subtitle_cues(&timeline::SubtitleTrack {
                id: name,
                text,
                playback_offset_ms: 0,
            })?;
            let any = !cues.is_empty();
            timeline::attach_cues(timeline, cues, false)?;
            Ok(any)
        });
        match attached {
            Err(reason) => note_subtitle_gap(timeline, format!("subtitle file {name}: {reason}")),
            Ok(true)
                if timeline
                    .clock
                    .as_ref()
                    .is_some_and(|c| c.origin_pts_us.is_none()) =>
            {
                // Sidecar cues carry their own playback clock; only embedded
                // tracks and cuts need the unknown media origin.
                let existing = timeline.components.subtitles.reason.take();
                timeline.components.subtitles = timeline::Component::partial(match existing {
                    Some(reason) => format!(
                        "{reason}; subtitle file {name} cues are attached on their own playback clock"
                    ),
                    None => format!("subtitle file {name} cues are attached on their own playback clock"),
                });
            }
            Ok(_) => {}
        }
    }
}

fn note_subtitle_gap(timeline: &mut timeline::Timeline, reason: String) {
    let current = &timeline.components.subtitles;
    let status = if current.status == timeline::Status::Unavailable {
        timeline::Status::Unavailable
    } else {
        timeline::Status::Partial
    };
    let reason = match &current.reason {
        Some(existing) => format!("{existing}; {reason}"),
        None => reason,
    };
    timeline.components.subtitles = timeline::Component {
        status,
        reason: Some(reason),
    };
}

/// Read one sidecar; every failure is a reason string, not a request error.
fn load_sidecar(path: &Path, policy: &SourceAccessPolicy) -> Result<String, String> {
    let admitted = policy.admit_path(&path.to_string_lossy())?;
    let file = std::fs::File::open(admitted).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("exceeds the 4 MiB sidecar limit".into());
    }
    String::from_utf8(bytes).map_err(|_| "is not valid UTF-8".into())
}

/// Only a fully complete manifest is cached; the key has no deadline, so a
/// deadline-limited partial result must never be reused.
fn cacheable(evidence: &VideoEvidence) -> bool {
    let components = &evidence.timeline.components;
    [
        &components.scenes,
        &components.chapters,
        &components.subtitles,
        &components.caption,
        &components.ocr,
        &components.transcript,
    ]
    .iter()
    .all(|c| {
        matches!(
            c.status,
            timeline::Status::Ok | timeline::Status::NotRequested
        )
    })
}

/// Private supervised self-worker entry, with no model download permission.
pub fn worker(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err("invalid video worker arguments".into());
    }
    let timeout = args[1]
        .strip_prefix("--supervised=")
        .ok_or("video supervision required")?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    crate::command_provider::supervise_parent(timeout)?;
    let work: Work = serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    println!("{}", execute(work)?);
    Ok(())
}
fn request(
    source: &str,
    selection: Option<TimelineSelection>,
    timestamps: Option<Vec<u64>>,
    transcript: bool,
    ocr: Option<crate::ocr_vlm::OcrEngine>,
    expected: Option<&str>,
    timeout: u64,
    policy: &SourceAccessPolicy,
) -> Result<Value, String> {
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
            let sidecar = Path::new(source).with_extension(extension);
            if sidecar.exists() {
                let text = load_sidecar(&sidecar, policy);
                sidecars.push((extension.into(), text));
            }
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
        Some(crate::ocr_evidence::OcrRequestPermit::acquire()?)
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
        Some(selection.clone()),
        None,
        transcript,
        ocr,
        None,
        60_000,
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
pub(crate) fn inspect(
    args: InspectArgs,
    policy: &SourceAccessPolicy,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let run = || -> Result<rmcp::model::CallToolResult, String> {
        if args.sources.len() != 1 {
            return Err("video operation requires exactly one source".into());
        }
        let source = &args.sources[0];
        let value = serde_json::to_value(source).map_err(|e| e.to_string())?;
        if value.as_object().is_some_and(|v| {
            v.iter()
                .any(|(k, v)| k != "path" && k != "url" && !v.is_null())
        }) {
            return Err("video source does not accept PDF page/region options".into());
        }
        let spec = match (&source.path, &source.url) {
            (Some(path), None) => path.as_str(),
            (None, Some(url)) => url.as_str(),
            _ => return Err("source requires path or URL, not both".into()),
        };
        if args.profile.is_some()
            || args.sample_pages.is_some()
            || args.scale.is_some()
            || args.max_pages.is_some()
            || args.max_regions.is_some()
            || args.max_pixels_per_page.is_some()
            || args.languages.is_some()
            || args.include_metadata.is_some()
        {
            return Err("PDF-only options are not applicable to video".into());
        }
        let timeline = matches!(args.operation, InspectOperation::VideoTimeline);
        if timeline
            && (args.timeline.is_none()
                || args.timestamps_ms.is_some()
                || args.include_image.unwrap_or(false))
        {
            return Err("video_timeline requires timeline and returns no image blocks".into());
        }
        if !timeline
            && (args.timeline.is_some()
                || args.timestamps_ms.is_none()
                || args.transcript.is_some()
                || args.ocr.is_some())
        {
            return Err("render_frame requires timestamps only".into());
        }
        let max = args.max_output_chars.unwrap_or(200_000) as usize;
        if !(1000..=1_000_000).contains(&max) {
            return Err("invalid max_output_chars".into());
        }
        let mut payload = request(
            spec,
            args.timeline,
            args.timestamps_ms,
            args.transcript.unwrap_or(false),
            args.ocr,
            args.expected_source_sha256.as_deref(),
            args.timeout_ms.unwrap_or(60_000) as u64,
            policy,
        )?;
        let mut images = Vec::new();
        if !timeline && args.include_image.unwrap_or(false) {
            for frame in payload["frames"].as_array().into_iter().flatten() {
                if let Some(path) = frame["image_path"].as_str() {
                    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                    use base64::Engine;
                    images.push(rmcp::model::ContentBlock::image(
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                        "image/png",
                    ));
                }
            }
        }
        if let Some(frames) = payload["frames"].as_array_mut() {
            for frame in frames {
                if let Some(object) = frame.as_object_mut() {
                    object.remove("image_path");
                }
            }
        }
        if serde_json::to_string(&payload)
            .map_err(|e| e.to_string())?
            .encode_utf16()
            .count()
            > max
        {
            return Err("video response exceeds output budget".into());
        }
        let source_hash = payload["source_sha256"]
            .as_str()
            .or_else(|| payload["timeline"]["source_sha256"].as_str())
            .map(str::to_string);
        let payload = crate::evidence::attach_evidence(
            "inspect",
            Some(if timeline {
                "video_timeline"
            } else {
                "render_frame"
            }),
            &[source.as_pdf_source()],
            "local-video",
            source_hash,
            vec![],
            payload,
        );
        let mut result = rmcp::model::CallToolResult::structured(payload);
        result.content.extend(images);
        Ok(result)
    };
    run().map_err(|message| rmcp::ErrorData::invalid_params(message, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operation_options_fail_before_source_or_worker_work() {
        for value in [
            json!({"operation":"video_timeline","sources":[{"path":"missing.mkv"}],"timeline":{"end_ms":0}}),
            json!({"operation":"video_timeline","sources":[{"path":"missing.mkv"}],"timeline":{"end_ms":1000},"include_image":true}),
            json!({"operation":"render_frame","sources":[{"path":"missing.mkv"}],"timestamps_ms":[]}),
            json!({"operation":"render_frame","sources":[{"path":"missing.mkv"}],"timestamps_ms":[0],"ocr":"vlm"}),
            json!({"operation":"render_frame","sources":[{"path":"missing.mkv","pages":"1"}],"timestamps_ms":[0]}),
        ] {
            let args: InspectArgs = serde_json::from_value(value).unwrap();
            let error = inspect(args, &SourceAccessPolicy::unrestricted()).unwrap_err();
            assert!(!error.message.contains("No such file"), "{:?}", error);
        }
    }
    #[test]
    fn malformed_sidecar_is_a_subtitles_gap_not_a_request_error() {
        let mut evidence = crate::video_evidence::tests::fixture();
        evidence.timeline.cues.clear();
        evidence.timeline.components.subtitles = timeline::Component::not_requested();
        attach_sidecars(
            &mut evidence.timeline,
            &[
                (
                    "srt".into(),
                    Ok("1\n00:00:01,000 --> 00:00:01,000\nzero".into()),
                ),
                (
                    "vtt".into(),
                    Ok("WEBVTT\n\n00:00.100 --> 00:00.200\nok".into()),
                ),
            ],
        );
        let subtitles = &evidence.timeline.components.subtitles;
        assert_eq!(subtitles.status, timeline::Status::Partial);
        assert!(subtitles
            .reason
            .as_deref()
            .unwrap()
            .contains("subtitle file srt"));
        assert_eq!(evidence.timeline.cues.len(), 1);
        evidence.timeline.components.subtitles = timeline::Component::gap("embedded unavailable");
        attach_sidecars(
            &mut evidence.timeline,
            &[(
                "vtt".into(),
                Ok("WEBVTT\n\n00:00.300 --> 00:00.400\nok".into()),
            )],
        );
        assert_eq!(
            evidence.timeline.components.subtitles.status,
            timeline::Status::Unavailable
        );
    }
    #[test]
    fn unreadable_sidecars_are_gaps_and_the_request_survives() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SourceAccessPolicy::unrestricted();
        let latin1 = dir.path().join("movie.srt");
        std::fs::write(&latin1, b"1\n00:00:01,000 --> 00:00:02,000\ncaf\xe9\n").unwrap();
        let reason = load_sidecar(&latin1, &policy).unwrap_err();
        assert!(reason.contains("UTF-8"), "{reason}");
        let big = dir.path().join("big.srt");
        std::fs::write(&big, vec![b'a'; 4 * 1024 * 1024 + 1]).unwrap();
        assert!(load_sidecar(&big, &policy).unwrap_err().contains("4 MiB"));
        assert!(load_sidecar(dir.path(), &policy).is_err());
        let mut evidence = crate::video_evidence::tests::fixture();
        evidence.timeline.cues.clear();
        evidence.timeline.components.subtitles = timeline::Component::not_requested();
        attach_sidecars(
            &mut evidence.timeline,
            &[
                ("srt".into(), Err(reason)),
                ("vtt".into(), Err("is a directory".into())),
            ],
        );
        let subtitles = &evidence.timeline.components.subtitles;
        assert_eq!(subtitles.status, timeline::Status::Partial);
        let text = subtitles.reason.as_deref().unwrap();
        assert!(text.contains("subtitle file srt") && text.contains("subtitle file vtt"));
    }
    #[test]
    fn sidecar_errors_keep_the_worse_status_and_earlier_reasons() {
        let mut evidence = crate::video_evidence::tests::fixture();
        evidence.timeline.components.subtitles = timeline::Component::gap("embedded unavailable");
        attach_sidecars(&mut evidence.timeline, &[("srt".into(), Err("bad".into()))]);
        let subtitles = &evidence.timeline.components.subtitles;
        assert_eq!(subtitles.status, timeline::Status::Unavailable);
        let text = subtitles.reason.as_deref().unwrap();
        assert!(text.contains("embedded unavailable") && text.contains("subtitle file srt: bad"));
    }
    #[test]
    fn unknown_origin_with_sidecar_cues_reports_what_is_attached() {
        let mut evidence = crate::video_evidence::tests::fixture();
        evidence.timeline.cues.clear();
        evidence.timeline.clock.as_mut().unwrap().origin_pts_us = None;
        evidence.timeline.components.subtitles = timeline::Component::gap(
            "media origin unavailable; embedded subtitle cues cannot be aligned",
        );
        attach_sidecars(
            &mut evidence.timeline,
            &[(
                "vtt".into(),
                Ok("WEBVTT\n\n00:00.300 --> 00:00.400\nok".into()),
            )],
        );
        let subtitles = &evidence.timeline.components.subtitles;
        assert_eq!(subtitles.status, timeline::Status::Partial);
        let text = subtitles.reason.as_deref().unwrap();
        assert!(
            text.contains("embedded") && text.contains("own playback clock"),
            "{text}"
        );
        assert!(!evidence.timeline.cues.is_empty());
    }
    #[test]
    fn invalid_transcript_cue_timing_is_a_gap_not_an_error() {
        let mut evidence = crate::video_evidence::tests::fixture();
        let bad = timeline::Cue {
            track: "asr".into(),
            start_ms: 5,
            end_ms: 5,
            text: "x".into(),
            timing: "segment".into(),
            provider: "t".into(),
        };
        assert!(timeline::attach_cues(&mut evidence.timeline, vec![bad.clone()], true).is_err());
        apply_transcript_cues(&mut evidence.timeline, vec![bad]);
        assert_eq!(
            evidence.timeline.components.transcript.status,
            timeline::Status::Unavailable
        );
    }
    #[test]
    fn partial_subtitles_or_chapters_are_not_cacheable() {
        let mut evidence = crate::video_evidence::tests::fixture();
        assert!(cacheable(&evidence));
        evidence.timeline.components.subtitles = timeline::Component::partial("deadline");
        assert!(!cacheable(&evidence));
        evidence.timeline.components.subtitles = timeline::Component::ok();
        evidence.timeline.components.chapters = timeline::Component::partial("deadline");
        assert!(!cacheable(&evidence));
    }
    #[test]
    fn additive_options_preserve_legacy_and_require_end() {
        assert!(serde_json::from_value::<TimelineSelection>(json!({"start_ms":0})).is_err());
        let selection: TimelineSelection = serde_json::from_value(json!({"end_ms":1000})).unwrap();
        assert_eq!(selection.max_scenes, 20);
        assert!(!selection.caption);
        let old: crate::schema::ReadArgs =
            serde_json::from_value(json!({"source":"x.pdf","ocr":true})).unwrap();
        assert!(old.timeline.is_none());
        assert!(old.ocr.unwrap().enabled());
        let old: crate::schema::OutlineArgs =
            serde_json::from_value(json!({"source":"x.pdf"})).unwrap();
        assert!(old.timeline.is_none());
    }
}
