//! Video evidence orchestration over already-admitted media. The shared request
//! owner supplies source identity, one deadline, budget, OCR permit and the
//! existing local-command caption/cache adapter. No parallel provider or cache.
use std::path::Path;
use std::time::Instant;

use anymd_formats::video::timeline::{
    self, Component, DecodedFrame, FrameMetadata, Status, Timeline, TimelineOptions,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterRegion {
    pub text: String,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    /// E.g. OCR region/word; never relabel layout regions as glyph geometry.
    pub geometry_level: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterObservation {
    pub provider: String,
    pub model_revision: Option<String>,
    pub coordinate_space: String,
    pub regions: Vec<RasterRegion>,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampledOcr {
    pub scene_id: String,
    pub frame: FrameMetadata,
    /// Supports this decoded frame only, not the entire scene interval.
    pub observation: RasterObservation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameDescription {
    pub text: String,
    pub provider: String,
    pub model_revision: Option<String>,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampledDescription {
    pub scene_id: String,
    pub frame: FrameMetadata,
    pub description: FrameDescription,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEvidence {
    pub timeline: Timeline,
    pub ocr_observations: Vec<SampledOcr>,
    pub descriptions: Vec<SampledDescription>,
}

/// Integration seam to the existing request owners, not a provider runner.
/// `caption_cached` must reuse the local-command region adapter only, never its
/// HTTP branch or doc-VLM. Successful descriptions are keyed by source/sidecar
/// hashes, resolved policy/options, frame hash and provider/model revision.
/// Failed/incomplete descriptions must not be cached as successful completion.
pub trait FrameContext {
    /// Checks the caller's shared deadline/exhaustion state before any work.
    fn ensure_available(&self, deadline: Instant) -> Result<(), String>;
    /// Charges the EXISTING request aggregate (40 MP and 32 MiB); no new permit.
    fn charge_frame(&mut self, pixels: u64, encoded_bytes: u64) -> Result<(), String>;
    /// A requested OCR operation without an installed/selected adapter is None.
    /// The adapter borrows the caller's existing OCR permit and raster entrypoint.
    fn ocr(
        &mut self,
        frame: &DecodedFrame,
        deadline: Instant,
    ) -> Result<Option<RasterObservation>, String>;
    /// True only for an explicitly configured local command vision adapter.
    fn local_caption_available(&self) -> bool;
    /// At most one successful cached caption per representative frame. Outline
    /// and render_frame never invoke this method; repeated reads reuse manifest.
    fn caption_cached(
        &mut self,
        source_hash: &str,
        scene_id: &str,
        frame: &DecodedFrame,
        deadline: Instant,
    ) -> Result<Option<FrameDescription>, String>;
}

fn validate_observation(
    observation: &RasterObservation,
    frame: &FrameMetadata,
) -> Result<(), String> {
    if observation.coordinate_space != "frame_pixels_top_left"
        || observation.provider.trim().is_empty()
    {
        return Err("video OCR requires frame-pixel coordinates and provider provenance".into());
    }
    for region in &observation.regions {
        if ![region.left, region.top, region.right, region.bottom]
            .iter()
            .all(|x| x.is_finite())
            || region.left < 0.0
            || region.top < 0.0
            || region.right <= region.left
            || region.bottom <= region.top
            || region.right > frame.width as f64
            || region.bottom > frame.height as f64
            || region.geometry_level.trim().is_empty()
        {
            return Err("video OCR supplied unsupported frame geometry".into());
        }
    }
    Ok(())
}
fn partial(reason: impl Into<String>) -> Component {
    Component {
        status: Status::Partial,
        reason: Some(reason.into()),
    }
}

/// Build the one authoritative timeline, then sample only representatives needed
/// for requested OCR/caption work. Images are never returned from this operation.
/// The caller caches this successful manifest and uses it for read/outline.
pub fn video_timeline(
    path: &Path,
    source_hash: &str,
    options: &TimelineOptions,
    ocr_requested: bool,
    deadline: Instant,
    context: &mut dyn FrameContext,
) -> Result<VideoEvidence, String> {
    context.ensure_available(deadline)?;
    let timeline = timeline::extract(path, source_hash, options, deadline)?;
    enrich(path, timeline, options, ocr_requested, deadline, context)
}

/// Enrich an extracted timeline after the shared owner attaches admitted sidecar
/// and ASR cues. This is also the stable integration point for worker results.
pub fn enrich(
    path: &Path,
    mut timeline: Timeline,
    options: &TimelineOptions,
    ocr_requested: bool,
    deadline: Instant,
    context: &mut dyn FrameContext,
) -> Result<VideoEvidence, String> {
    options.validate()?;
    if timeline.requested.start_ms != options.start_ms
        || timeline.requested.end_ms != options.end_ms
        || timeline.scenes.len() > options.max_scenes
    {
        return Err("timeline manifest does not match resolved request options".into());
    }
    let caption_requested = options.caption && context.local_caption_available();
    timeline.components.ocr = if ocr_requested {
        Component::gap("no sampled OCR observation")
    } else {
        Component::not_requested()
    };
    timeline.components.caption = if !options.caption {
        Component::not_requested()
    } else if !caption_requested {
        Component::gap("configured local-command caption adapter unavailable")
    } else {
        Component::gap("no sampled frame description")
    };
    let mut evidence = VideoEvidence {
        timeline,
        ocr_observations: vec![],
        descriptions: vec![],
    };
    if !ocr_requested && !caption_requested {
        return Ok(evidence);
    }
    let Some(clock) = evidence.timeline.clock.clone() else {
        return Ok(evidence);
    };
    let mut ocr_gap = false;
    let mut caption_gap = false;
    for index in 0..evidence.timeline.scenes.len() {
        let scene = &evidence.timeline.scenes[index];
        let scene_id = scene.id.clone();
        let requested = scene.start_ms;
        let end = scene.end_ms;
        let result = (|| {
            context.ensure_available(deadline)?;
            let frame = timeline::render_frame(path, &clock, requested, end, deadline)?;
            let Some(frame) = frame else {
                return Ok(None);
            };
            context.charge_frame(
                frame.metadata.width as u64 * frame.metadata.height as u64,
                frame.png.len() as u64,
            )?;
            evidence.timeline.scenes[index].representative = Some(frame.metadata.clone());
            if ocr_requested {
                context.ensure_available(deadline)?;
                match context.ocr(&frame, deadline)? {
                    Some(observation) => {
                        validate_observation(&observation, &frame.metadata)?;
                        ocr_gap |= observation.truncated;
                        evidence.ocr_observations.push(SampledOcr {
                            scene_id: scene_id.clone(),
                            frame: frame.metadata.clone(),
                            observation,
                        });
                    }
                    None => {
                        ocr_gap = true;
                    }
                }
            }
            if caption_requested {
                context.ensure_available(deadline)?;
                match context.caption_cached(
                    &evidence.timeline.source_sha256,
                    &scene_id,
                    &frame,
                    deadline,
                )? {
                    Some(description) => {
                        if description.text.trim().is_empty()
                            || description.provider != "external-command"
                        {
                            return Err(
                                "caption requires nonempty local-command description provenance"
                                    .into(),
                            );
                        }
                        caption_gap |= description.truncated;
                        evidence.descriptions.push(SampledDescription {
                            scene_id,
                            frame: frame.metadata.clone(),
                            description,
                        });
                    }
                    None => {
                        caption_gap = true;
                    }
                }
            }
            Ok::<_, String>(Some(()))
        })();
        match result {
            Ok(Some(())) => {}
            Ok(None) => {
                ocr_gap |= ocr_requested;
                caption_gap |= caption_requested;
            }
            Err(message) => {
                // No subsequent frame/OCR/caption worker may start after a
                // decode/admission/provider/budget failure or expired deadline.
                if ocr_requested {
                    evidence.timeline.components.ocr = partial(&message);
                }
                if caption_requested {
                    evidence.timeline.components.caption = partial(message);
                }
                return Ok(evidence);
            }
        }
    }
    if ocr_requested {
        evidence.timeline.components.ocr = if evidence.ocr_observations.is_empty() {
            Component::gap("requested OCR produced no sampled observations")
        } else if ocr_gap {
            partial("sampled OCR is incomplete or truncated")
        } else {
            Component::ok()
        };
    }
    if caption_requested {
        evidence.timeline.components.caption = if evidence.descriptions.is_empty() {
            Component::gap("local caption adapter produced no descriptions")
        } else if caption_gap {
            partial("sampled descriptions are incomplete or truncated")
        } else {
            Component::ok()
        };
    }
    Ok(evidence)
}

/// On-demand frames only: never OCR/caption, never a nearest keyframe labeled as
/// the requested time. Source/hash matching and response image blocks stay with
/// the shared inspect owner. Batch uses the same borrowed aggregate and deadline.
pub fn render_frames(
    path: &Path,
    timeline: &Timeline,
    timestamps_ms: &[u64],
    deadline: Instant,
    context: &mut dyn FrameContext,
) -> Result<Vec<Option<DecodedFrame>>, String> {
    if timestamps_ms.is_empty() || timestamps_ms.len() > 20 {
        return Err("timestamps_ms must contain between 1 and 20 positions".into());
    }
    if timestamps_ms
        .iter()
        .any(|x| *x < timeline.requested.start_ms || *x >= timeline.requested.end_ms)
    {
        return Err("frame timestamp must lie in the admitted half-open interval".into());
    }
    let clock = timeline.clock.as_ref().ok_or("video clock unavailable")?;
    let mut frames = Vec::with_capacity(timestamps_ms.len());
    for timestamp in timestamps_ms {
        context.ensure_available(deadline)?;
        let frame =
            timeline::render_frame(path, clock, *timestamp, timeline.requested.end_ms, deadline)?;
        if let Some(ref frame) = frame {
            context.charge_frame(
                frame.metadata.width as u64 * frame.metadata.height as u64,
                frame.png.len() as u64,
            )?;
        }
        frames.push(frame);
    }
    Ok(frames)
}

/// Stable document-section projection for read/outline. It does not execute any
/// decoder/provider and does not change headings based on generated descriptions.
pub fn sections(evidence: &VideoEvidence) -> Vec<anymd_formats::Section> {
    let mut sections = Vec::new();
    for scene in &evidence.timeline.scenes {
        let mut text = format!(
            "Playback interval: [{} ms, {} ms). Detection: {} ({})",
            scene.start_ms, scene.end_ms, scene.detection, evidence.timeline.version
        );
        let cues: Vec<_> = evidence
            .timeline
            .cues
            .iter()
            .enumerate()
            .filter(|(_, cue)| {
                cue.start_ms < scene.end_ms as i64 && cue.end_ms > scene.start_ms as i64
            })
            .map(|(index, cue)| {
                format!(
                    "- Cue {} (track {}) [{} ms, {} ms); text in timeline cues section",
                    index + 1,
                    cue.track,
                    cue.start_ms,
                    cue.end_ms
                )
            })
            .collect();
        if !cues.is_empty() {
            text.push_str(
                "\n\nOverlapping cues (original endpoints; not inferred scene speech):\n",
            );
            text.push_str(&cues.join("\n"));
        }
        for sample in evidence
            .ocr_observations
            .iter()
            .filter(|x| x.scene_id == scene.id)
        {
            text.push_str(&format!(
                "\n\nSampled OCR at {} ms (frame {}), not continuous scene coverage:\n{}",
                sample.frame.actual_ms,
                sample.frame.sha256,
                sample
                    .observation
                    .regions
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        for sample in evidence
            .descriptions
            .iter()
            .filter(|x| x.scene_id == scene.id)
        {
            text.push_str(&format!(
                "\n\nSampled-frame description at {} ms ({}): {}",
                sample.frame.actual_ms, sample.description.provider, sample.description.text
            ));
        }
        sections.push(anymd_formats::Section {
            label: format!("scene {}: {}–{} ms", scene.id, scene.start_ms, scene.end_ms),
            markdown: text,
        });
    }
    if !evidence.timeline.cues.is_empty() {
        let markdown = evidence
            .timeline
            .cues
            .iter()
            .enumerate()
            .map(|(index, cue)| {
                format!(
                    "- Cue {} (track {}, {}) [{} ms, {} ms): {}",
                    index + 1,
                    cue.track,
                    cue.timing,
                    cue.start_ms,
                    cue.end_ms,
                    cue.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(anymd_formats::Section {
            label: "timeline cues".into(),
            markdown,
        });
    }
    for chapter in &evidence.timeline.chapters {
        sections.push(anymd_formats::Section {
            label: format!("chapter {}", chapter.id),
            markdown: format!(
                "{}\n\nPlayback interval: [{} ms, {} ms).",
                chapter.title.as_deref().unwrap_or("Untitled chapter"),
                chapter.start_ms,
                chapter.end_ms
            ),
        });
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_geometry_never_assumes_pdf_page() {
        let frame = FrameMetadata {
            requested_ms: 0,
            actual_ms: 17,
            decoded_pts: 17,
            decoded_pts_us: 17_000,
            stream: 0,
            time_base: "1/1000".into(),
            width: 100,
            height: 50,
            sha256: "hash".into(),
        };
        let mut observation = RasterObservation {
            provider: "ocr".into(),
            model_revision: Some("revision".into()),
            coordinate_space: "frame_pixels_top_left".into(),
            regions: vec![RasterRegion {
                text: "observed".into(),
                left: 0.,
                top: 0.,
                right: 100.,
                bottom: 50.,
                geometry_level: "ocr_region".into(),
            }],
            truncated: false,
        };
        assert!(validate_observation(&observation, &frame).is_ok());
        observation.regions[0].right = 101.;
        assert!(validate_observation(&observation, &frame).is_err());
        observation.coordinate_space = "pdf_points".into();
        assert!(validate_observation(&observation, &frame).is_err());
    }
}
