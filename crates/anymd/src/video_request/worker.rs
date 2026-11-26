use super::cache::{cache_get, cache_put, cacheable, hash, remaining};
use super::client::TimelineSelection;
use super::sidecars::{apply_transcript_cues, attach_sidecars};
use crate::video_evidence::{
    self, FrameContext, FrameDescription, RasterObservation, VideoEvidence,
};
use anymd_formats::video::timeline::{self, DecodedFrame};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
pub(super) struct Work {
    pub(super) path: PathBuf,
    pub(super) source_hash: String,
    /// Each sidecar load outcome; a load error is a subtitles gap, not a failed request.
    pub(super) sidecars: Vec<(String, Result<String, String>)>,
    pub(super) selection: Option<TimelineSelection>,
    pub(super) timestamps: Option<Vec<u64>>,
    pub(super) transcript: bool,
    pub(super) ocr: Option<crate::ocr_vlm::OcrEngine>,
    pub(super) timeout_ms: u64,
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
