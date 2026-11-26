use std::{
    ffi::OsString,
    path::Path,
    time::{Duration, Instant},
};

use serde_json::Value;

use super::types::{Chapter, MediaClock, Scene, TimeRange, LOG_BYTES, METADATA_BYTES};
use crate::tool;

pub(super) fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|x| !x.is_zero())
        .ok_or_else(|| "video request deadline exhausted".into())
}
pub(super) fn args(items: &[&str]) -> Vec<OsString> {
    items.iter().map(|item| OsString::from(*item)).collect()
}
pub(super) fn seconds(us: i64) -> String {
    format!("{:.6}", us as f64 / 1_000_000.0)
}
fn micros(value: &Value) -> Option<i64> {
    let seconds = value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())?;
    if !seconds.is_finite() || seconds.abs() >= i64::MAX as f64 / 1_000_000.0 {
        return None;
    }
    Some((seconds * 1_000_000.0).round() as i64)
}

/// Parse metadata separately from execution, allowing inert fixtures to prove
/// offsets and timing. The format start is authoritative; missing is not zero.
pub fn metadata(probe: &Value) -> Result<(MediaClock, Option<u64>, Vec<Chapter>), String> {
    let streams = probe["streams"]
        .as_array()
        .ok_or("ffprobe missing streams")?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        .ok_or("source has no video stream")?;
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");
    let clock = MediaClock {
        origin_pts_us: micros(&probe["format"]["start_time"]),
        video_start_pts_us: micros(&video["start_time"]),
        audio_start_pts_us: audio.and_then(|s| micros(&s["start_time"])),
        video_stream: video["index"]
            .as_u64()
            .and_then(|x| u32::try_from(x).ok())
            .ok_or("invalid video stream index")?,
        video_time_base: video["time_base"]
            .as_str()
            .filter(|x| x.split_once('/').is_some())
            .ok_or("video time base unavailable")?
            .into(),
        audio_stream: audio
            .and_then(|s| s["index"].as_u64())
            .and_then(|x| u32::try_from(x).ok()),
    };
    let duration = micros(&probe["format"]["duration"])
        .filter(|x| *x > 0)
        .map(|x| x as u64 / 1000);
    let mut chapters = Vec::new();
    if clock.origin_pts_us.is_some() {
        for ch in probe["chapters"].as_array().into_iter().flatten() {
            let start = micros(&ch["start_time"]).and_then(|x| clock.playback_us(x));
            let end = micros(&ch["end_time"]).and_then(|x| clock.playback_us(x));
            if let (Some(a), Some(b)) = (start, end) {
                if b > a {
                    chapters.push(Chapter {
                        id: ch["id"].to_string(),
                        start_ms: a / 1000,
                        end_ms: b / 1000,
                        title: ch["tags"]["title"].as_str().map(str::to_owned),
                    });
                }
            }
        }
    }
    Ok((clock, duration, chapters))
}

/// Read actual showinfo PTS, never inferred from requested seek time or keyframes.
pub fn decoded_pts(log: &str) -> Result<Vec<(i64, i64)>, String> {
    let mut result = Vec::new();
    for line in log
        .lines()
        .filter(|l| l.contains("showinfo") && l.contains("pts_time:"))
    {
        let field = |key: &str| {
            line.split_once(key)
                .and_then(|(_, s)| s.split_whitespace().next())
        };
        let pts = field("pts:")
            .and_then(|x| x.parse::<i64>().ok())
            .ok_or("invalid decoded PTS")?;
        let us = field("pts_time:")
            .and_then(|x| micros(&Value::String(x.into())))
            .ok_or("invalid decoded PTS time")?;
        result.push((pts, us));
    }
    Ok(result)
}

/// The filter's actual clock, not an assumed codec/keyframe clock.
pub fn decoded_time_base(log: &str) -> Result<String, String> {
    log.lines()
        .filter(|line| line.contains("showinfo"))
        .find_map(|line| {
            line.split_once("config in time_base:")
                .and_then(|(_, suffix)| suffix.split(',').next())
        })
        .map(str::trim)
        .filter(|value| value.split_once('/').is_some())
        .map(str::to_owned)
        .ok_or_else(|| "decoded frame filter time base unavailable".into())
}

pub(super) fn exact_pts_us(ticks: i64, time_base: &str) -> Result<i64, String> {
    let (num, den) = time_base
        .split_once('/')
        .ok_or("invalid decoded time base")?;
    let num: i128 = num
        .parse()
        .map_err(|_| "invalid decoded time base numerator")?;
    let den: i128 = den
        .parse()
        .map_err(|_| "invalid decoded time base denominator")?;
    if num <= 0 || den <= 0 {
        return Err("decoded time base must be positive".into());
    }
    let value = (ticks as i128)
        .checked_mul(num)
        .and_then(|x| x.checked_mul(1_000_000))
        .ok_or("decoded timestamp overflow")?
        / den;
    i64::try_from(value).map_err(|_| "decoded timestamp overflow".into())
}

/// Scene construction caps boundaries, not coverage: overflow merges the tail
/// into the last scene and explicitly marks detection partial.
pub fn scenes_from_pts(
    clock: &MediaClock,
    range: &TimeRange,
    cuts: &[(i64, i64)],
    max: usize,
) -> (Vec<Scene>, bool) {
    let mut boundaries = vec![range.start_ms];
    boundaries.extend(
        cuts.iter()
            .filter_map(|(_, pts_us)| clock.playback_us(*pts_us))
            .filter(|x| *x > 0)
            .map(|x| (x as u64).div_ceil(1000))
            .filter(|x| *x > range.start_ms && *x < range.end_ms),
    );
    boundaries.sort_unstable();
    boundaries.dedup();
    let truncated = boundaries.len() > max;
    boundaries.truncate(max);
    let scenes = boundaries
        .iter()
        .enumerate()
        .map(|(n, start)| Scene {
            id: format!("scene-{start}"),
            start_ms: *start,
            end_ms: boundaries.get(n + 1).copied().unwrap_or(range.end_ms),
            detection: if n == 0 {
                "initial_window_scene"
            } else {
                "detected_cut"
            }
            .into(),
            representative: None,
        })
        .collect();
    (scenes, truncated)
}

pub(super) fn read_probe(ffprobe: &Path, path: &Path, deadline: Instant) -> Result<Value, String> {
    let mut probe_args = args(&[
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
        "-show_chapters",
        "--",
    ]);
    probe_args.push(path.as_os_str().into());
    let output = tool::run_bounded(
        ffprobe,
        probe_args,
        remaining(deadline)?,
        METADATA_BYTES,
        LOG_BYTES,
    )?;
    if !output.success {
        return Err("ffprobe could not read the admitted media source".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "invalid ffprobe metadata".into())
}

/// Clock-only lookup for standalone render_frame. None means ffprobe absent;
/// no scene extraction or inference is performed.
pub fn probe_clock(path: &Path, deadline: Instant) -> Result<Option<MediaClock>, String> {
    remaining(deadline)?;
    let Some(ffprobe) = tool::find("ffprobe") else {
        return Ok(None);
    };
    metadata(&read_probe(&ffprobe, path, deadline)?).map(|(clock, _, _)| Some(clock))
}
