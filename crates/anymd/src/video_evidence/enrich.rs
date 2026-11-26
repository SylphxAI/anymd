use std::path::Path;
use std::time::Instant;

use anymd_formats::video::timeline::{
    self, Component, FrameMetadata, Status, Timeline, TimelineOptions,
};

use super::{FrameContext, RasterObservation, SampledDescription, SampledOcr, VideoEvidence};

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

#[cfg(test)]
#[path = "enrich_tests.rs"]
mod tests;
