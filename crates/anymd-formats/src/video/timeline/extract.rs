use std::{path::Path, time::Instant};

use super::probe::{args, decoded_pts, metadata, read_probe, remaining, scenes_from_pts, seconds};
use super::subtitles::extract_subtitles;
use super::types::{
    base, Component, Status, TimeRange, Timeline, TimelineOptions, LOG_BYTES, METADATA_BYTES,
};
use crate::tool;

/// Extract only scene/chapter metadata. No images or inference. `path` must be
/// the same admitted immutable representation identified by `source_sha256`.
pub fn extract(
    path: &Path,
    source_sha256: &str,
    options: &TimelineOptions,
    deadline: Instant,
) -> Result<Timeline, String> {
    options.validate()?;
    remaining(deadline)?;
    let mut timeline = base(source_sha256, options);
    let Some(ffprobe) = tool::find("ffprobe") else {
        timeline.components.scenes = Component::gap("ffprobe unavailable");
        timeline.components.chapters = Component::gap("ffprobe unavailable");
        timeline.components.subtitles = Component::gap("ffprobe unavailable");
        return Ok(timeline);
    };
    let probe = read_probe(&ffprobe, path, deadline)?;
    let (clock, duration, chapters) = metadata(&probe)?;
    timeline.chapters = chapters
        .into_iter()
        .filter(|c| c.start_ms < options.end_ms as i64 && c.end_ms > options.start_ms as i64)
        .collect();
    timeline.components.chapters = if clock.origin_pts_us.is_some() {
        Component::ok()
    } else {
        Component::gap("media origin unavailable")
    };
    timeline.clock = Some(clock.clone());
    if clock.origin_pts_us.is_none() {
        timeline.components.scenes =
            Component::gap("media origin unavailable; detected cuts cannot be aligned");
        timeline.components.subtitles =
            Component::gap("media origin unavailable; embedded subtitle cues cannot be aligned");
        return Ok(timeline);
    }
    let end = duration.map_or(options.end_ms, |d| d.min(options.end_ms));
    if end <= options.start_ms {
        return Err("requested interval does not intersect the media duration".into());
    }
    let mut range = TimeRange {
        start_ms: options.start_ms,
        end_ms: end,
    };
    let Some(ffmpeg) = tool::find("ffmpeg") else {
        timeline.components.scenes =
            Component::gap("ffmpeg unavailable; chapters are not detected cuts");
        timeline.components.subtitles = Component::gap("ffmpeg unavailable for embedded subtitles");
        return Ok(timeline);
    };
    let start = seconds(clock.absolute_us(range.start_ms)?);
    let end_time = seconds(clock.absolute_us(range.end_ms)?);
    let filter =
        format!("trim=start={start}:end={end_time},select='eq(n\\,0)+gt(scene\\,0.4)',showinfo");
    let mut ffargs = args(&[
        "-nostdin",
        "-v",
        "info",
        "-protocol_whitelist",
        "file,pipe",
        "-copyts",
        "-i",
    ]);
    ffargs.push(path.as_os_str().into());
    ffargs.extend(args(&[
        "-map",
        &format!("0:{}", clock.video_stream),
        "-an",
        "-sn",
        "-vf",
        &filter,
        "-fps_mode",
        "passthrough",
        "-f",
        "null",
        "-",
    ]));
    let output = tool::run_bounded(
        &ffmpeg,
        ffargs,
        remaining(deadline)?,
        METADATA_BYTES,
        LOG_BYTES,
    )?;
    if !output.success {
        timeline.components.scenes = Component::gap("FFmpeg detected-cut extraction failed");
        return Ok(timeline);
    }
    let cuts = decoded_pts(&String::from_utf8_lossy(&output.stderr))?;
    let Some((_, first_pts)) = cuts.first() else {
        timeline.components.scenes = Component::gap("no decodable frame within requested interval");
        return Ok(timeline);
    };
    let first_us = clock
        .playback_us(*first_pts)
        .ok_or("initial scene origin unavailable")?;
    if first_us > 0 {
        range.start_ms = range.start_ms.max((first_us as u64).div_ceil(1000));
    }
    if range.start_ms >= range.end_ms {
        timeline.components.scenes = Component::gap("no decodable frame within requested interval");
        return Ok(timeline);
    }
    let (scenes, truncated) = scenes_from_pts(&clock, &range, &cuts[1..], options.max_scenes);
    timeline.scenes = scenes;
    timeline.processed = Some(range);
    timeline.components.scenes = if truncated {
        Component {
            status: Status::Partial,
            reason: Some("detected cut count exceeded max_scenes; tail scenes coalesced".into()),
        }
    } else {
        Component::ok()
    };
    if let Err(reason) = extract_subtitles(path, &probe, &mut timeline, deadline) {
        timeline.components.subtitles = Component {
            status: Status::Partial,
            reason: Some(reason),
        };
    }
    Ok(timeline)
}
