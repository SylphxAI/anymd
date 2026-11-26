//! Transcript cue data and the clock-aware worker entry point.

#[cfg(feature = "native")]
use std::path::Path;

#[cfg(feature = "native")]
use super::super::timeline;
#[cfg(feature = "native")]
use super::pipeline::transcribe_structured;

#[derive(Debug, PartialEq)]
pub(in crate::video) struct TranscriptCue {
    pub(in crate::video) start_ms: u64,
    pub(in crate::video) end_ms: u64,
    pub(in crate::video) text: String,
    pub(in crate::video) word: bool,
}

#[cfg(feature = "native")]
pub(super) struct TranscriptWindow<'a> {
    pub(super) clock: &'a timeline::MediaClock,
    pub(super) options: &'a timeline::TimelineOptions,
    pub(super) deadline: std::time::Instant,
}

/// Structured windowed ASR for the existing isolated request worker. Model
/// loading is native/uncancellable, so the shared owner invokes this through
/// its self-worker boundary, never an abandoned spawn_blocking task. No download.
#[cfg(feature = "native")]
pub fn transcript_window(
    path: &Path,
    clock: &timeline::MediaClock,
    options: &timeline::TimelineOptions,
    deadline: std::time::Instant,
) -> Result<Vec<timeline::Cue>, String> {
    options.validate()?;
    if clock.origin_pts_us.is_none()
        || clock.audio_stream.is_none()
        || clock.audio_start_pts_us.is_none()
    {
        return Err("audio stream clock/offset unavailable; transcript cannot be aligned".into());
    }
    let window = TranscriptWindow {
        clock,
        options,
        deadline,
    };
    transcribe_structured(path, false, Some(&window)).map(|cues| {
        cues.into_iter()
            .map(|c| timeline::Cue {
                track: format!("asr:{}", clock.audio_stream.unwrap_or_default()),
                start_ms: c.start_ms as i64,
                end_ms: c.end_ms as i64,
                text: c.text,
                timing: if c.word { "word" } else { "segment" }.into(),
                provider: if c.word {
                    "qwen3_asr_qwen3_forced_aligner"
                } else {
                    "qwen3_asr_20_second_segments"
                }
                .into(),
            })
            .collect()
    })
}
