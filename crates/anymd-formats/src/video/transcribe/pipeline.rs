//! Local ASR orchestration, deadlines, and word-to-segment fallback.

use std::path::Path;
#[cfg(feature = "native")]
use std::{path::PathBuf, time::Duration};

#[cfg(feature = "native")]
use super::super::PROBE_TIMEOUT;
#[cfg(feature = "native")]
use super::align::align_words;
#[cfg(feature = "native")]
use super::audio::{extract_chunk, AudioChunk, CHUNK_SECONDS, SAMPLES_PER_CHUNK};
#[cfg(feature = "native")]
use super::window::TranscriptWindow;
#[cfg(feature = "native")]
use super::{render_transcript, TranscriptCue};
#[cfg(feature = "native")]
use crate::{asr, tool};

#[cfg(feature = "native")]
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(1800);

#[cfg(not(feature = "native"))]
pub(in crate::video) fn transcribe(_path: &Path, _download: bool) -> Result<String, String> {
    Err("transcripts need the native anymd build".into())
}

#[cfg(feature = "native")]
pub(in crate::video) fn transcribe(path: &Path, download: bool) -> Result<String, String> {
    transcribe_structured(path, download, None).map(|cues| render_transcript(&cues))
}

#[cfg(feature = "native")]
pub(super) fn transcribe_structured(
    path: &Path,
    download: bool,
    window: Option<&TranscriptWindow<'_>>,
) -> Result<Vec<TranscriptCue>, String> {
    use transcribe_cpp::{Backend, CancelToken, Model, ModelOptions, RunOptions, SessionOptions};

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
            let Some(AudioChunk {
                pcm,
                duration_ms,
                cue_offset_ms,
            }) = extract_chunk(&ffmpeg, path, &wav, window, offset_ms, chunk_end_ms, budget)?
            else {
                if window.is_some() {
                    offset_ms = chunk_end_ms;
                    continue;
                }
                break;
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
