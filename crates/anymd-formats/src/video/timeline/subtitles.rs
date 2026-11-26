use std::{path::Path, time::Instant};

use serde_json::Value;

use super::probe::{args, remaining};
use super::types::{Component, Cue, Status, SubtitleTrack, Timeline, LOG_BYTES, METADATA_BYTES};
use crate::{
    tool,
    video::{clean_cue_line, parse_timestamp},
};

/// Preserve both endpoints, rolling repeats, overlaps and cue order. No Markdown
/// round-trip. Unknown WebVTT timestamp maps fail rather than inventing alignment.
pub fn subtitle_cues(track: &SubtitleTrack<'_>) -> Result<Vec<Cue>, String> {
    if track.text.contains("X-TIMESTAMP-MAP") {
        return Err("WebVTT timestamp map needs explicit resolved media alignment".into());
    }
    let text = track
        .text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<_> = block.lines().collect();
        let Some(idx) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let (a, b) = lines[idx]
            .split_once("-->")
            .ok_or("invalid subtitle timing")?;
        let start = parse_timestamp(a)
            .and_then(|x| i64::try_from(x).ok())
            .and_then(|x| x.checked_add(track.playback_offset_ms))
            .ok_or("invalid subtitle start")?;
        let end = parse_timestamp(b)
            .and_then(|x| i64::try_from(x).ok())
            .and_then(|x| x.checked_add(track.playback_offset_ms))
            .ok_or("invalid subtitle end")?;
        if end <= start {
            return Err("subtitle end must follow start".into());
        }
        let text = lines[idx + 1..]
            .iter()
            .map(|x| clean_cue_line(x))
            .collect::<Vec<_>>()
            .join("\n");
        cues.push(Cue {
            track: track.id.into(),
            start_ms: start,
            end_ms: end,
            text,
            timing: "subtitle_cue".into(),
            provider: "srt_webvtt".into(),
        });
    }
    if cues.is_empty() && !text.trim().is_empty() && text.trim() != "WEBVTT" {
        return Err("no structured subtitle cues".into());
    }
    Ok(cues)
}

/// Attach admitted ASR cues (word or segment granularity retained). Inference is
/// owned by the existing request worker, not this module's demuxer pipeline.
pub fn attach_cues(
    timeline: &mut Timeline,
    cues: Vec<Cue>,
    transcript: bool,
) -> Result<(), String> {
    if cues.iter().any(|c| {
        c.end_ms <= c.start_ms || !matches!(c.timing.as_str(), "subtitle_cue" | "word" | "segment")
    }) {
        return Err("invalid structured cue timing".into());
    }
    timeline.cues.extend(cues.into_iter().filter(|c| {
        c.start_ms < timeline.requested.end_ms as i64
            && c.end_ms > timeline.requested.start_ms as i64
    }));
    timeline.cues.sort_by_key(|c| (c.start_ms, c.end_ms));
    if transcript {
        timeline.components.transcript = Component::ok();
    } else if timeline.components.subtitles.status == Status::NotRequested {
        // Never overwrite a partial or unavailable result with a later success.
        timeline.components.subtitles = Component::ok();
    }
    Ok(())
}

pub(super) fn extract_subtitles(
    path: &Path,
    probe: &Value,
    timeline: &mut Timeline,
    deadline: Instant,
) -> Result<(), String> {
    let clock = timeline
        .clock
        .as_ref()
        .ok_or("subtitle media clock unavailable")?
        .clone();
    let origin = clock
        .origin_pts_us
        .ok_or("subtitle media origin unavailable")?
        / 1000;
    let ffmpeg = tool::find("ffmpeg").ok_or("ffmpeg unavailable")?;
    let tracks: Vec<_> = probe["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["codec_type"] == "subtitle")
        .collect();
    let mut failed = tracks.len() > 4;
    for track in tracks.into_iter().take(4) {
        let Some(idx) = track["index"].as_u64() else {
            failed = true;
            continue;
        };
        let mut ffargs = args(&[
            "-nostdin",
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-copyts",
            "-i",
        ]);
        ffargs.push(path.as_os_str().into());
        ffargs.extend(args(&[
            "-map",
            &format!("0:{idx}"),
            "-c:s",
            "srt",
            "-f",
            "srt",
            "pipe:1",
        ]));
        let output = tool::run_bounded(
            &ffmpeg,
            ffargs,
            remaining(deadline)?,
            METADATA_BYTES,
            LOG_BYTES,
        )?;
        if !output.success {
            failed = true;
            continue;
        }
        let text = String::from_utf8(output.stdout).map_err(|_| "subtitle text is not UTF-8")?;
        let id = format!("embedded:{idx}");
        let cues = subtitle_cues(&SubtitleTrack {
            id: &id,
            text: &text,
            playback_offset_ms: -origin,
        });
        match cues {
            Ok(mut cues) => {
                for cue in &mut cues {
                    cue.provider = "ffmpeg_srt_copyts".into();
                }
                attach_cues(timeline, cues, false)?;
            }
            Err(_) => {
                failed = true;
            }
        }
    }
    timeline.components.subtitles = if failed {
        Component {
            status: Status::Partial,
            reason: Some("one or more subtitle tracks unavailable or track budget exceeded".into()),
        }
    } else {
        Component::ok()
    };
    Ok(())
}
