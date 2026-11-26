use crate::source_access::SourceAccessPolicy;
use anymd_formats::video::timeline;
use std::io::Read;
use std::path::Path;

pub(super) fn apply_transcript_cues(timeline: &mut timeline::Timeline, cues: Vec<timeline::Cue>) {
    if let Err(reason) = timeline::attach_cues(timeline, cues, true) {
        timeline.components.transcript =
            timeline::Component::gap(format!("transcript cues rejected: {reason}"));
    }
}

/// A subtitle file error (load or parse) is a gap on that component, never a
/// failed request. Reasons accumulate and the worse status is kept.
pub(super) fn attach_sidecars(
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

/// Read one media sidecar under the allowlist: admitted first, then opened.
pub(super) fn sidecar_text(
    source: &Path,
    extension: &str,
    policy: &SourceAccessPolicy,
) -> Result<String, String> {
    let sidecar = source.with_extension(extension);
    policy.admit_path(&sidecar.to_string_lossy())?;
    load_sidecar(&sidecar, policy)
}

/// Read one sidecar; every failure is a reason string, not a request error.
pub(super) fn load_sidecar(path: &Path, policy: &SourceAccessPolicy) -> Result<String, String> {
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
