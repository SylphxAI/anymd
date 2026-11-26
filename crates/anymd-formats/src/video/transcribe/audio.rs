//! FFmpeg extraction, PCM decoding, and chunk placement on the media clock.

use std::{path::Path, time::Duration};

use super::super::timeline;
use super::window::TranscriptWindow;
use crate::tool;

const AUDIO_EXTRACT_TIMEOUT: Duration = Duration::from_secs(600);
pub(super) const CHUNK_SECONDS: u64 = 20;
pub(super) const SAMPLES_PER_CHUNK: usize = 16_000 * CHUNK_SECONDS as usize;

pub(super) struct AudioChunk {
    pub(super) pcm: Vec<f32>,
    pub(super) duration_ms: u64,
    pub(super) cue_offset_ms: u64,
}

pub(super) fn extract_chunk(
    ffmpeg: &Path,
    path: &Path,
    wav: &Path,
    window: Option<&TranscriptWindow<'_>>,
    offset_ms: u64,
    chunk_end_ms: u64,
    budget: impl Fn(Duration) -> Result<Duration, String>,
) -> Result<Option<AudioChunk>, String> {
    let seek = format!("{:.3}", offset_ms as f64 / 1000.0);
    let output = if let Some(w) = window {
        let origin = w.clock.origin_pts_us.ok_or("audio origin unavailable")?;
        let absolute = |ms: u64| origin as f64 / 1_000_000.0 + ms as f64 / 1000.0;
        let filter = format!(
            "atrim=start={:.6}:end={:.6},aresample=16000,ashowinfo",
            absolute(offset_ms),
            absolute(chunk_end_ms)
        );
        let map = format!(
            "0:{}",
            w.clock.audio_stream.ok_or("audio stream unavailable")?
        );
        tool::run_bounded(
            ffmpeg,
            [
                std::ffi::OsStr::new("-nostdin"),
                "-loglevel".as_ref(),
                "info".as_ref(),
                "-y".as_ref(),
                "-protocol_whitelist".as_ref(),
                "file,pipe".as_ref(),
                "-copyts".as_ref(),
                "-i".as_ref(),
                path.as_os_str(),
                "-map".as_ref(),
                map.as_ref(),
                "-af".as_ref(),
                filter.as_ref(),
                "-vn".as_ref(),
                "-ac".as_ref(),
                "1".as_ref(),
                "-ar".as_ref(),
                "16000".as_ref(),
                "-acodec".as_ref(),
                "pcm_s16le".as_ref(),
                wav.as_os_str(),
            ],
            budget(AUDIO_EXTRACT_TIMEOUT)?,
            1024 * 1024,
            2 * 1024 * 1024,
        )?
    } else {
        tool::run(
            ffmpeg,
            [
                std::ffi::OsStr::new("-nostdin"),
                "-loglevel".as_ref(),
                "error".as_ref(),
                "-y".as_ref(),
                "-ss".as_ref(),
                seek.as_ref(),
                "-i".as_ref(),
                path.as_os_str(),
                "-t".as_ref(),
                "20".as_ref(),
                "-vn".as_ref(),
                "-ac".as_ref(),
                "1".as_ref(),
                "-ar".as_ref(),
                "16000".as_ref(),
                "-acodec".as_ref(),
                "pcm_s16le".as_ref(),
                wav.as_os_str(),
            ],
            AUDIO_EXTRACT_TIMEOUT,
        )?
    };
    if !output.success {
        return Err("ffmpeg could not extract the audio track".into());
    }
    if window.is_some() && std::fs::metadata(wav).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("audio extraction exceeded window output budget".into());
    }
    let mut reader = hound::WavReader::open(wav).map_err(|e| format!("audio chunk: {e}"))?;
    let pcm = reader
        .samples::<i16>()
        .take(SAMPLES_PER_CHUNK + 1)
        .map(|s| s.map(|s| s as f32 / 32768.0))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("audio samples: {e}"))?;
    if pcm.is_empty() {
        return Ok(None);
    }
    if pcm.len() > SAMPLES_PER_CHUNK {
        return Err("ffmpeg exceeded the audio chunk limit".into());
    }
    let duration_ms = pcm.len() as u64 * 1000 / 16_000;
    let cue_offset_ms = if let Some(w) = window {
        let log = String::from_utf8_lossy(&output.stderr);
        let pts = timeline::decoded_pts(&log)?;
        let (_, first) = pts.first().ok_or("audio samples have no PTS alignment")?;
        let playback = w
            .clock
            .playback_us(*first)
            .ok_or("audio clock unavailable")?;
        if playback < 0 || playback < offset_ms as i64 * 1000 {
            return Err("audio extraction timestamps precede requested window".into());
        }
        let actual = playback as u64 / 1000;
        if actual.saturating_add(duration_ms) > chunk_end_ms + 1 {
            return Err("audio chunk exceeds requested window".into());
        }
        actual
    } else {
        offset_ms
    };
    Ok(Some(AudioChunk {
        pcm,
        duration_ms,
        cue_offset_ms,
    }))
}
