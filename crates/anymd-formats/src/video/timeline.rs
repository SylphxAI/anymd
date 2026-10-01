//! Opt-in bounded media evidence. Sources/sidecars are admitted and hashed by
//! the caller; this module never fetches URLs, finds sidecars, or installs models.
//! FFmpeg's detected cuts are heuristic, not semantic scene verification.
use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::tool;

pub const POLICY: &str = "ffmpeg_detected_cuts_v1_threshold_0.4";
pub const MAX_WINDOW_MS: u64 = 600_000;
pub const MAX_SCENES: usize = 20;
pub const MAX_FRAME_PIXELS: u64 = 2_000_000;
pub const MAX_FRAME_BYTES: u64 = 4 * 1024 * 1024;
const METADATA_BYTES: u64 = 1024 * 1024;
const LOG_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TimelineOptions {
    #[serde(default)]
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default = "default_scenes")]
    pub max_scenes: usize,
    #[serde(default)]
    pub caption: bool,
}
fn default_scenes() -> usize {
    MAX_SCENES
}
impl TimelineOptions {
    pub fn validate(&self) -> Result<(), String> {
        if self.end_ms <= self.start_ms
            || self.end_ms > i64::MAX as u64 / 1000
            || self.end_ms - self.start_ms > MAX_WINDOW_MS
        {
            return Err(
                "timeline requires a positive half-open interval of at most 600000 ms".into(),
            );
        }
        if self.max_scenes == 0 || self.max_scenes > MAX_SCENES {
            return Err("max_scenes must be between 1 and 20".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Partial,
    Unavailable,
    NotRequested,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub status: Status,
    pub reason: Option<String>,
}
impl Component {
    pub fn ok() -> Self {
        Self {
            status: Status::Ok,
            reason: None,
        }
    }
    pub fn gap(reason: impl Into<String>) -> Self {
        Self {
            status: Status::Unavailable,
            reason: Some(reason.into()),
        }
    }
    pub fn not_requested() -> Self {
        Self {
            status: Status::NotRequested,
            reason: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start_ms: u64,
    pub end_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaClock {
    /// Absolute demuxer PTS corresponding to playback time zero. None is a gap.
    pub origin_pts_us: Option<i64>,
    pub video_start_pts_us: Option<i64>,
    pub audio_start_pts_us: Option<i64>,
    pub video_stream: u32,
    pub video_time_base: String,
    pub audio_stream: Option<u32>,
}
impl MediaClock {
    pub fn playback_us(&self, pts_us: i64) -> Option<i64> {
        pts_us.checked_sub(self.origin_pts_us?)
    }
    fn absolute_us(&self, playback_ms: u64) -> Result<i64, String> {
        let millis = i64::try_from(playback_ms).map_err(|_| "media time overflow")?;
        self.origin_pts_us
            .ok_or("media origin timestamp unavailable")?
            .checked_add(millis.checked_mul(1000).ok_or("media time overflow")?)
            .ok_or_else(|| "media time overflow".into())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub title: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    pub track: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub timing: String,
    pub provider: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameMetadata {
    pub requested_ms: u64,
    /// Rounded playback milliseconds; exact decoded PTS is also retained.
    pub actual_ms: i64,
    pub decoded_pts: i64,
    pub decoded_pts_us: i64,
    pub stream: u32,
    pub time_base: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}
#[derive(Debug)]
pub struct DecodedFrame {
    pub metadata: FrameMetadata,
    pub png: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    /// Stable identity independent of OCR or generated description text.
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub detection: String,
    pub representative: Option<FrameMetadata>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    pub version: String,
    pub source_sha256: String,
    pub requested: TimeRange,
    pub processed: Option<TimeRange>,
    pub clock: Option<MediaClock>,
    pub scenes: Vec<Scene>,
    pub chapters: Vec<Chapter>,
    /// Once per track, not copied or stretched into each scene.
    pub cues: Vec<Cue>,
    pub components: Components,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Components {
    pub scenes: Component,
    pub chapters: Component,
    pub subtitles: Component,
    pub transcript: Component,
    pub ocr: Component,
    pub caption: Component,
}

/// A caller-admitted UTF-8 sidecar. Clock offsets come from the admitted track,
/// not from a filename guess. WebVTT MPEGTS mapping must be resolved by caller.
pub struct SubtitleTrack<'a> {
    pub id: &'a str,
    pub text: &'a str,
    pub playback_offset_ms: i64,
}

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
        let start = super::parse_timestamp(a)
            .and_then(|x| i64::try_from(x).ok())
            .and_then(|x| x.checked_add(track.playback_offset_ms))
            .ok_or("invalid subtitle start")?;
        let end = super::parse_timestamp(b)
            .and_then(|x| i64::try_from(x).ok())
            .and_then(|x| x.checked_add(track.playback_offset_ms))
            .ok_or("invalid subtitle end")?;
        if end <= start {
            return Err("subtitle end must follow start".into());
        }
        let text = lines[idx + 1..]
            .iter()
            .map(|x| super::clean_cue_line(x))
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
    } else {
        timeline.components.subtitles = Component::ok();
    }
    Ok(())
}

fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|x| !x.is_zero())
        .ok_or_else(|| "video request deadline exhausted".into())
}
fn args(items: &[&str]) -> Vec<OsString> {
    items.iter().map(|item| OsString::from(*item)).collect()
}
fn seconds(us: i64) -> String {
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

fn exact_pts_us(ticks: i64, time_base: &str) -> Result<i64, String> {
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

fn base(hash: &str, options: &TimelineOptions) -> Timeline {
    Timeline {
        version: POLICY.into(),
        source_sha256: hash.into(),
        requested: TimeRange {
            start_ms: options.start_ms,
            end_ms: options.end_ms,
        },
        processed: None,
        clock: None,
        scenes: vec![],
        chapters: vec![],
        cues: vec![],
        components: Components {
            scenes: Component::gap("not yet extracted"),
            chapters: Component::gap("not yet extracted"),
            subtitles: Component::not_requested(),
            transcript: Component::not_requested(),
            ocr: Component::not_requested(),
            caption: if options.caption {
                Component::gap("local caption adapter unavailable")
            } else {
                Component::not_requested()
            },
        },
    }
}

fn read_probe(ffprobe: &Path, path: &Path, deadline: Instant) -> Result<Value, String> {
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
            Component::gap("media origin unavailable; subtitle cues cannot be aligned");
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

fn extract_subtitles(
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

/// First decodable frame >= requested playback timestamp, within a caller-bound
/// half-open interval. No seek approximation, PTS reset, captioning or caching.
pub fn render_frame(
    path: &Path,
    clock: &MediaClock,
    requested_ms: u64,
    end_ms: u64,
    deadline: Instant,
) -> Result<Option<DecodedFrame>, String> {
    if end_ms <= requested_ms || end_ms - requested_ms > MAX_WINDOW_MS {
        return Err("frame interval must be positive and at most ten minutes".into());
    }
    let ffmpeg = tool::find("ffmpeg").ok_or("ffmpeg unavailable")?;
    let start = seconds(clock.absolute_us(requested_ms)?);
    let end = seconds(clock.absolute_us(end_ms)?);
    // Bound even pathological portrait/landscape sources before PNG encoding.
    let filter = format!(
        "trim=start={start}:end={end},scale=w='min(iw,1414)':h='min(ih,1414)':force_original_aspect_ratio=decrease,showinfo"
    );
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
        "-frames:v",
        "1",
        "-fps_mode",
        "passthrough",
        "-c:v",
        "png",
        "-f",
        "image2pipe",
        "pipe:1",
    ]));
    let output = tool::run_bounded(
        &ffmpeg,
        ffargs,
        remaining(deadline)?,
        MAX_FRAME_BYTES,
        LOG_BYTES,
    )?;
    if !output.success {
        return Err("FFmpeg frame decode failed".into());
    }
    if output.stdout.is_empty() {
        return Ok(None);
    }
    let pts = decoded_pts(&String::from_utf8_lossy(&output.stderr))?;
    // showinfo can observe a lookahead frame. The first record belongs to the
    // one encoded PNG; never choose a later timestamp from the log.
    let (ticks, _) = *pts.first().ok_or("decoded PNG has no timestamp evidence")?;
    let time_base = decoded_time_base(&String::from_utf8_lossy(&output.stderr))?;
    let pts_us = exact_pts_us(ticks, &time_base)?;
    let playback_us = clock
        .playback_us(pts_us)
        .ok_or("frame origin unavailable")?;
    if playback_us < i64::try_from(requested_ms).map_err(|_| "time overflow")? * 1000
        || playback_us >= i64::try_from(end_ms).map_err(|_| "time overflow")? * 1000
    {
        return Err("decoded frame outside requested interval".into());
    }
    let size = imagesize::blob(&output.stdout).map_err(|_| "invalid decoded PNG")?;
    if size.width == 0
        || size.height == 0
        || (size.width as u64) * (size.height as u64) > MAX_FRAME_PIXELS
    {
        return Err("decoded frame exceeds pixel budget".into());
    }
    let sha256 = format!("{:x}", Sha256::digest(&output.stdout));
    Ok(Some(DecodedFrame {
        metadata: FrameMetadata {
            requested_ms,
            actual_ms: playback_us / 1000,
            decoded_pts: ticks,
            decoded_pts_us: pts_us,
            stream: clock.video_stream,
            time_base,
            width: size.width as u32,
            height: size.height as u32,
            sha256,
        },
        png: output.stdout,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn clock() -> MediaClock {
        metadata(
            &json!({"format":{"start_time":"5.0", "duration":"3.0"},"streams":[
            {"index":0,"codec_type":"video","start_time":"5.2","time_base":"1/1000"},
            {"index":1,"codec_type":"audio","start_time":"5.0"}]}),
        )
        .unwrap()
        .0
    }
    #[test]
    fn timestamps_offsets_vfr_and_initial_scene() {
        let c = clock();
        assert_eq!(c.video_start_pts_us, Some(5_200_000));
        assert_eq!(c.audio_start_pts_us, Some(5_000_000));
        let pts = decoded_pts("[Parsed_showinfo_2] n: 0 pts: 5200 pts_time:5.2\n[Parsed_showinfo_2] n: 1 pts: 5537 pts_time:5.537").unwrap();
        assert_eq!(c.playback_us(pts[1].1), Some(537_000));
        let (scenes, partial) = scenes_from_pts(
            &c,
            &TimeRange {
                start_ms: 0,
                end_ms: 3000,
            },
            &pts,
            2,
        );
        assert!(partial);
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[1].end_ms, 3000);
    }
    #[test]
    fn subtitles_keep_end_overlap_repeat_and_cross_cut() {
        let track = SubtitleTrack {
            id: "english",
            playback_offset_ms: 200,
            text: "WEBVTT\n\n00:00.100 --> 00:01.537\nrepeat\n\n00:00.900 --> 00:02.100\nrepeat",
        };
        let cues = subtitle_cues(&track).unwrap();
        assert_eq!((cues[0].start_ms, cues[0].end_ms), (300, 1737));
        assert_eq!(cues[0].text, cues[1].text);
        assert!(cues[1].start_ms < cues[0].end_ms);
    }
    #[test]
    fn unknown_origin_is_not_zero() {
        let (clock, _, _) = metadata(
            &json!({"format":{}, "streams":[{"index":0,"codec_type":"video","time_base":"1/90"}]}),
        )
        .unwrap();
        assert!(clock.playback_us(0).is_none());
    }
    #[test]
    fn exact_filter_timebase_and_unresolved_subtitle_map() {
        let log = "[Parsed_showinfo_2] config in time_base: 1/90000, frame_rate: 25/1";
        assert_eq!(decoded_time_base(log).unwrap(), "1/90000");
        assert_eq!(exact_pts_us(465300, "1/90000").unwrap(), 5_170_000);
        assert!(exact_pts_us(1, "1/0").is_err());
        assert!(decoded_time_base("pts_time:5.17").is_err());
        let track = SubtitleTrack {
            id: "mapped",
            playback_offset_ms: 0,
            text: "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00.000,MPEGTS:450000\n\n00:00.100 --> 00:00.200\ntext",
        };
        assert!(subtitle_cues(&track).is_err());
    }
    #[test]
    fn strict_window_and_scene_limits() {
        let mut options = TimelineOptions {
            start_ms: 0,
            end_ms: 600_000,
            max_scenes: 20,
            caption: false,
        };
        assert!(options.validate().is_ok());
        options.end_ms += 1;
        assert!(options.validate().is_err());
        options.end_ms = 1;
        options.max_scenes = 0;
        assert!(options.validate().is_err());
        assert!(serde_json::from_str::<TimelineOptions>("{\"start_ms\":0}").is_err());
    }
}
