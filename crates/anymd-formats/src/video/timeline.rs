//! Opt-in bounded media evidence. Sources/sidecars are admitted and hashed by
//! the caller; this module never fetches URLs, finds sidecars, or installs models.
//! FFmpeg's detected cuts are heuristic, not semantic scene verification.

mod extract;
mod probe;
mod render;
mod subtitles;
mod types;

pub use extract::extract;
pub use probe::{decoded_pts, decoded_time_base, metadata, probe_clock, scenes_from_pts};
pub use render::render_frame;
pub use subtitles::{attach_cues, subtitle_cues};
pub use types::{
    Chapter, Component, Components, Cue, DecodedFrame, FrameMetadata, MediaClock, Scene, Status,
    SubtitleTrack, TimeRange, Timeline, TimelineOptions, MAX_FRAME_BYTES, MAX_FRAME_PIXELS,
    MAX_SCENES, MAX_WINDOW_MS, POLICY,
};

#[cfg(test)]
mod tests;
