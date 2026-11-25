//! Local ASR orchestration, word alignment, and transcript rendering.

use std::path::Path;
#[cfg(feature = "native")]
use std::path::PathBuf;
#[cfg(feature = "native")]
use std::time::Duration;

use serde_json::Value;

#[cfg(feature = "native")]
use super::{timeline, PROBE_TIMEOUT};
#[cfg(feature = "native")]
use crate::{asr, tool};

#[cfg(feature = "native")]
const AUDIO_EXTRACT_TIMEOUT: Duration = Duration::from_secs(600);
#[cfg(feature = "native")]
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(1800);

#[cfg(not(feature = "native"))]
pub(super) fn transcribe(_path: &Path, _download: bool) -> Result<String, String> {
    Err("transcripts need the native anymd build".into())
}

#[derive(Debug, PartialEq)]
pub(super) struct TranscriptCue {
    pub(super) start_ms: u64,
    pub(super) end_ms: u64,
    pub(super) text: String,
    pub(super) word: bool,
}

#[cfg(feature = "native")]
pub(super) fn transcribe(path: &Path, download: bool) -> Result<String, String> {
    transcribe_structured(path, download, None).map(|cues| render_transcript(&cues))
}

#[cfg(feature = "native")]
struct TranscriptWindow<'a> {
    clock: &'a timeline::MediaClock,
    options: &'a timeline::TimelineOptions,
    deadline: std::time::Instant,
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

#[cfg(feature = "native")]
fn transcribe_structured(
    path: &Path,
    download: bool,
    window: Option<&TranscriptWindow<'_>>,
) -> Result<Vec<TranscriptCue>, String> {
    use transcribe_cpp::{Backend, CancelToken, Model, ModelOptions, RunOptions, SessionOptions};
    const CHUNK_SECONDS: u64 = 20;
    const SAMPLES_PER_CHUNK: usize = 16_000 * CHUNK_SECONDS as usize;

    let budget = |normal: Duration| -> Result<Duration, String> {
        match window {
            Some(w) => w
                .deadline
                .checked_duration_since(std::time::Instant::now())
                .filter(|d| !d.is_zero())
                .map(|d| d.min(normal))
                .ok_or_else(|| "video transcript deadline exhausted".into()),
            None => Ok(normal),
        }
    };
    budget(TRANSCRIBE_TIMEOUT)?;
    let ffmpeg = tool::find("ffmpeg")
        .ok_or_else(|| format!("needs ffmpeg; install with {}", asr::ffmpeg_hint()))?;
    // No model download if the audio extraction tool is missing.
    let model_path = asr::ensure_model(&asr::ASR, asr::MODEL_ENV, download)?;
    let model = Model::load_with(
        &model_path,
        &ModelOptions {
            backend: Backend::Cpu,
            ..Default::default()
        },
    )
    .map_err(|e| format!("Qwen3-ASR model load: {e}"))?;
    let mut session = model
        .session_with(&SessionOptions {
            n_threads: 4,
            ..Default::default()
        })
        .map_err(|e| format!("Qwen3-ASR session: {e}"))?;
    let cancel = CancelToken::new();
    session.set_cancel_token(&cancel);
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let timer_cancel = cancel.clone();
    let transcript_timeout = budget(TRANSCRIBE_TIMEOUT)?;
    let timer = std::thread::spawn(move || {
        if matches!(
            wait.recv_timeout(transcript_timeout),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            timer_cancel.cancel();
        }
    });
    // Every exit drops the sender, releasing the timer (including errors).
    let result = (|| {
        let dir = tempfile::tempdir().map_err(|e| format!("temp dir: {e}"))?;
        let wav = dir.path().join("chunk.wav");
        let mut cues = Vec::new();
        let mut offset_ms = window.map_or(0, |w| w.options.start_ms);
        let mut aligner = None;
        let mut alignment_disabled = false;
        loop {
            if cancel.is_cancelled() {
                return Err("Qwen3-ASR transcript timed out".into());
            }
            if window.is_some_and(|w| offset_ms >= w.options.end_ms) {
                break;
            }
            let chunk_end_ms = window.map_or(offset_ms.saturating_add(CHUNK_SECONDS * 1000), |w| {
                w.options.end_ms.min(offset_ms + CHUNK_SECONDS * 1000)
            });
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
                    &ffmpeg,
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
                    &ffmpeg,
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
            if window.is_some()
                && std::fs::metadata(&wav).map_err(|e| e.to_string())?.len() > 1024 * 1024
            {
                return Err("audio extraction exceeded window output budget".into());
            }
            let mut reader =
                hound::WavReader::open(&wav).map_err(|e| format!("audio chunk: {e}"))?;
            let pcm = reader
                .samples::<i16>()
                .take(SAMPLES_PER_CHUNK + 1)
                .map(|s| s.map(|s| s as f32 / 32768.0))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("audio samples: {e}"))?;
            if pcm.is_empty() {
                if window.is_some() {
                    offset_ms = chunk_end_ms;
                    continue;
                }
                break;
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
            budget(TRANSCRIBE_TIMEOUT)?;
            // Avoid hallucination on digitally silent chunks, without a speech-volume threshold.
            if pcm.iter().any(|s| *s != 0.0) {
                let transcript = session
                    .run(&pcm, &RunOptions::default())
                    .map_err(|e| format!("Qwen3-ASR inference: {e}"))?;
                let text = transcript.text.trim();
                if !text.is_empty() {
                    let language = transcript.language.as_deref().unwrap_or("");
                    let supported = matches!(
                        language,
                        "en" | "zh" | "yue" | "ja" | "ko" | "fr" | "de" | "it" | "pt" | "ru" | "es"
                    );
                    if supported && !alignment_disabled && aligner.is_none() {
                        // CrispASR is used ONLY for standalone ForcedAligner, never ASR.
                        let binary = std::env::var_os(asr::ALIGNER_BIN_ENV)
                            .map(PathBuf::from)
                            .or_else(|| tool::find("crispasr"));
                        if let Some(binary) = binary {
                            match asr::ensure_model(&asr::ALIGNER, asr::ALIGNER_ENV, download) {
                                Ok(weights) => aligner = Some((binary, weights)),
                                Err(e) => {
                                    eprintln!("anymd: word alignment unavailable: {e}");
                                    alignment_disabled = true;
                                }
                            }
                        } else {
                            alignment_disabled = true;
                        }
                    }
                    let align_timeout = budget(PROBE_TIMEOUT)?;
                    let aligned = if supported && !alignment_disabled {
                        aligner.as_ref().and_then(|(binary, weights)| {
                            match align_words(
                                binary,
                                weights,
                                &wav,
                                text,
                                duration_ms,
                                align_timeout,
                                window.map(|_| 1024 * 1024),
                            ) {
                                Ok(words) => Some(words),
                                Err(e) => {
                                    eprintln!("anymd: word alignment unavailable: {e}");
                                    alignment_disabled = true;
                                    None
                                }
                            }
                        })
                    } else {
                        None
                    };
                    if let Some(words) = aligned {
                        cues.extend(words.into_iter().map(|mut cue| {
                            cue.start_ms += cue_offset_ms;
                            cue.end_ms += cue_offset_ms;
                            cue
                        }));
                    } else {
                        cues.push(TranscriptCue {
                            start_ms: cue_offset_ms,
                            end_ms: cue_offset_ms + duration_ms,
                            text: text.into(),
                            word: false,
                        });
                    }
                }
            }
            if pcm.len() < SAMPLES_PER_CHUNK && window.is_none() {
                break;
            }
            offset_ms = chunk_end_ms;
        }
        budget(TRANSCRIBE_TIMEOUT)?;
        Ok(cues)
    })();
    drop(done);
    let _ = timer.join();
    result
}

#[cfg(feature = "native")]
fn align_words(
    binary: &Path,
    model: &Path,
    wav: &Path,
    text: &str,
    duration_ms: u64,
    timeout: Duration,
    output_limit: Option<u64>,
) -> Result<Vec<TranscriptCue>, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let reference = dir.path().join("reference.txt");
    std::fs::write(&reference, text).map_err(|e| e.to_string())?;
    let args = [
        std::ffi::OsStr::new("--align-only"),
        "-am".as_ref(),
        model.as_os_str(),
        "-f".as_ref(),
        wav.as_os_str(),
        "--text-file".as_ref(),
        reference.as_os_str(),
        "--align-format".as_ref(),
        "json".as_ref(),
        "--align-granularity".as_ref(),
        "word".as_ref(),
        "-t".as_ref(),
        "4".as_ref(),
        "-ng".as_ref(),
    ];
    let output = match output_limit {
        Some(limit) => tool::run_bounded(binary, args, timeout, limit, 64 * 1024)?,
        None => tool::run(binary, args, timeout)?,
    };

    if !output.success {
        return Err("ForcedAligner failed; retaining segment timestamps".into());
    }
    let payload = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    parse_alignment(&payload, text, duration_ms)
}

pub(super) fn parse_alignment(
    payload: &str,
    text: &str,
    duration_ms: u64,
) -> Result<Vec<TranscriptCue>, String> {
    let root: Value =
        serde_json::from_str(payload).map_err(|e| format!("ForcedAligner JSON: {e}"))?;
    let rows = root
        .as_array()
        .ok_or("ForcedAligner JSON must be a word array")?;
    let mut cues = Vec::new();
    let mut previous_end = 0u64;
    for row in rows {
        let start = row
            .get("start")
            .and_then(Value::as_f64)
            .ok_or("missing word start")?;
        let end = row
            .get("end")
            .and_then(Value::as_f64)
            .ok_or("missing word end")?;
        let word = row
            .get("word")
            .and_then(Value::as_str)
            .ok_or("missing alignment word")?;
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || end < start
            || end * 1000.0 > duration_ms as f64
        {
            return Err("word timestamps outside audio".into());
        }
        let start_ms = (start * 1000.0).round() as u64;
        let end_ms = (end * 1000.0).round() as u64;
        if start_ms < previous_end || word.trim().is_empty() {
            return Err("invalid word alignment".into());
        }
        previous_end = end_ms;
        cues.push(TranscriptCue {
            start_ms,
            end_ms,
            text: word.into(),
            word: true,
        });
    }
    let letters = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let aligned = cues.iter().map(|c| c.text.as_str()).collect::<String>();
    if cues.is_empty() || letters(&aligned) != letters(text) {
        return Err("word alignment omitted or changed transcript text".into());
    }
    Ok(cues)
}

pub(super) fn render_transcript(cues: &[TranscriptCue]) -> String {
    if cues.is_empty() {
        return String::new();
    }
    let kind = if cues.iter().all(|c| c.word) {
        "word (Qwen3-ForcedAligner)"
    } else if cues.iter().any(|c| c.word) {
        "mixed word and segment"
    } else {
        "segment (20-second chunk boundaries, not word timing)"
    };
    let clock = |ms: u64| {
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            ms / 3_600_000,
            ms / 60_000 % 60,
            ms / 1000 % 60,
            ms % 1000
        )
    };
    let lines = cues
        .iter()
        .map(|c| format!("[{} --> {}] {}", clock(c.start_ms), clock(c.end_ms), c.text))
        .collect::<Vec<_>>()
        .join("\n");
    format!("_Timestamps: {kind}._\n\n{lines}")
}
