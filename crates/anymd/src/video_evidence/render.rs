use std::path::Path;
use std::time::Instant;

use anymd_formats::video::timeline::{self, DecodedFrame};

use super::{FrameContext, VideoEvidence};

/// On-demand frames only: never OCR/caption, never a nearest keyframe labeled as
/// the requested time. Source/hash matching and response image blocks stay with
/// the shared inspect owner. Batch uses the same borrowed aggregate and deadline.
pub fn render_frames(
    path: &Path,
    clock: &timeline::MediaClock,
    timestamps_ms: &[u64],
    deadline: Instant,
    context: &mut dyn FrameContext,
) -> Result<Vec<Option<DecodedFrame>>, String> {
    if timestamps_ms.is_empty() || timestamps_ms.len() > 20 {
        return Err("timestamps_ms must contain between 1 and 20 positions".into());
    }
    if timestamps_ms.iter().any(|x| {
        x.checked_add(timeline::MAX_WINDOW_MS).is_none()
            || *x > i64::MAX as u64 / 1000 - timeline::MAX_WINDOW_MS
    }) {
        return Err("frame timestamp exceeds media-clock limits".into());
    }
    let mut frames = Vec::with_capacity(timestamps_ms.len());
    for timestamp in timestamps_ms {
        context.ensure_available(deadline)?;
        let frame = timeline::render_frame(
            path,
            clock,
            *timestamp,
            *timestamp + timeline::MAX_WINDOW_MS,
            deadline,
        )?;
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

/// Generated text is data, never document structure: a fence longer than any
/// backtick run inside keeps `#` or underline lines from becoming outline nodes.
fn fenced(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}text\n{text}\n{fence}")
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
                fenced(
                    &sample
                        .observation
                        .regions
                        .iter()
                        .map(|r| r.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            ));
        }
        for sample in evidence
            .descriptions
            .iter()
            .filter(|x| x.scene_id == scene.id)
        {
            text.push_str(&format!(
                "\n\nSampled-frame description at {} ms ({}):\n{}",
                sample.frame.actual_ms,
                sample.description.provider,
                fenced(&sample.description.text)
            ));
        }
        sections.push(anymd_formats::Section {
            label: format!("scene {}: {}–{} ms", scene.id, scene.start_ms, scene.end_ms),
            markdown: text,
        });
    }
    for chapter in &evidence.timeline.chapters {
        sections.push(anymd_formats::Section {
            label: format!("chapter {}", chapter.id),
            markdown: format!(
                "{}\n\nPlayback interval: [{} ms, {} ms).",
                fenced(chapter.title.as_deref().unwrap_or("Untitled chapter")),
                chapter.start_ms,
                chapter.end_ms
            ),
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
                    "- Cue {} (track {}, {}) [{} ms, {} ms):\n{}",
                    index + 1,
                    cue.track,
                    cue.timing,
                    cue.start_ms,
                    cue.end_ms,
                    fenced(&cue.text)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(anymd_formats::Section {
            label: "timeline cues".into(),
            markdown,
        });
    }
    sections
}
