use serde::{Deserialize, Serialize};

pub const POLICY: &str = "ffmpeg_detected_cuts_v1_threshold_0.4";
pub const MAX_WINDOW_MS: u64 = 600_000;
pub const MAX_SCENES: usize = 20;
pub const MAX_FRAME_PIXELS: u64 = 2_000_000;
pub const MAX_FRAME_BYTES: u64 = 4 * 1024 * 1024;
pub(super) const METADATA_BYTES: u64 = 1024 * 1024;
pub(super) const LOG_BYTES: u64 = 2 * 1024 * 1024;

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
    pub fn partial(reason: impl Into<String>) -> Self {
        Self {
            status: Status::Partial,
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
    pub(super) fn absolute_us(&self, playback_ms: u64) -> Result<i64, String> {
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

pub(super) fn base(hash: &str, options: &TimelineOptions) -> Timeline {
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
