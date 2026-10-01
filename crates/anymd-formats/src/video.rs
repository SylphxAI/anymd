//! Audio/video → Markdown: container facts, streams, chapters, subtitles, and an
//! opt-in transcript. Ported from cue `video-reader-core` (ffprobe, subtitle
//! extraction, local ASR orchestration), reduced to what reads well as text.
//!
//! Everything degrades: without ffprobe the output is the sniffed container plus
//! an install hint; without ffmpeg the transcript is a how-to line.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::asr;
use crate::{ConvertError, Converted, Options, Section, tool};

/// Bounded, opt-in video evidence; legacy conversion is unchanged.
#[cfg(feature = "native")]
pub mod timeline;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const SUBTITLE_TIMEOUT: Duration = Duration::from_secs(60);
const AUDIO_EXTRACT_TIMEOUT: Duration = Duration::from_secs(600);
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(1800);
const MAX_SIDECAR_BYTES: u64 = 10 * 1024 * 1024;

/// True when the leading bytes identify this media kind.
pub fn sniff(head: &[u8]) -> bool {
    container(head).is_some()
}

/// Container name and a file extension ffmpeg recognises, from magic bytes.
fn container(head: &[u8]) -> Option<(&'static str, &'static str)> {
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        let brand = &head[8..12];
        if crate::image::is_image_brand(brand) {
            return None;
        }
        return Some(match brand {
            b"M4A " | b"M4B " => ("MPEG-4 audio", "m4a"),
            b"qt  " => ("QuickTime", "mov"),
            _ => ("MPEG-4", "mp4"),
        });
    }
    if head.len() >= 8
        && matches!(&head[4..8], b"moov" | b"mdat" | b"wide" | b"free" | b"skip")
        && u32::from_be_bytes([head[0], head[1], head[2], head[3]]) >= 8
    {
        return Some(("QuickTime", "mov"));
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        let window = &head[..head.len().min(64)];
        return Some(if window.windows(4).any(|w| w == b"webm") {
            ("WebM", "webm")
        } else {
            ("Matroska", "mkv")
        });
    }
    if head.len() >= 12 && head.starts_with(b"RIFF") {
        return match &head[8..12] {
            b"AVI " => Some(("AVI", "avi")),
            b"WAVE" => Some(("WAV", "wav")),
            _ => None,
        };
    }
    if head.starts_with(b"ID3") {
        return Some(("MP3", "mp3"));
    }
    if head.starts_with(b"fLaC") {
        return Some(("FLAC", "flac"));
    }
    if head.starts_with(b"OggS") {
        return Some(("Ogg", "ogg"));
    }
    if head.starts_with(b"FLV\x01") {
        return Some(("FLV", "flv"));
    }
    if head.len() >= 377 && head[0] == 0x47 && head[188] == 0x47 && head[376] == 0x47 {
        return Some(("MPEG-TS", "ts"));
    }
    if head.len() >= 4 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0 {
        let version = (head[1] >> 3) & 0b11;
        let layer = (head[1] >> 1) & 0b11;
        if layer == 0 && head[1] & 0xF6 == 0xF0 {
            return Some(("AAC (ADTS)", "aac"));
        }
        let bitrate = head[2] >> 4;
        let sample_rate = (head[2] >> 2) & 0b11;
        if version != 1 && layer != 0 && bitrate != 0b1111 && sample_rate != 0b11 {
            return Some(("MP3", "mp3"));
        }
    }
    None
}

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    let sniffed = container(bytes);
    let given_path = options.path.as_deref().filter(|p| p.is_file());

    let Some(ffprobe) = tool::find("ffprobe") else {
        let (name, _) = sniffed
            .ok_or_else(|| ConvertError::Invalid("not a recognized audio/video file".into()))?;
        return Ok(basic(
            name,
            bytes.len() as u64,
            given_path,
            if cfg!(feature = "native") {
                "_Install ffmpeg (`ffprobe`) to report duration, streams, chapters, and subtitles._"
            } else {
                "_The browser build reports the container only; the anymd CLI with ffmpeg adds duration, streams, chapters, and subtitles._"
            },
        ));
    };

    // ffprobe needs a path: the caller's file, or the bytes in a temp file.
    let temp = match given_path {
        Some(_) => None,
        None => {
            let ext = sniffed.map(|(_, ext)| ext).unwrap_or("bin");
            Some(tool::temp_file(bytes, &format!(".{ext}")).map_err(ConvertError::Unsupported)?)
        }
    };
    let path: &Path = match (&temp, given_path) {
        (Some(file), _) => file.path(),
        (None, Some(path)) => path,
        (None, None) => return Err(ConvertError::Invalid("no media path".into())),
    };

    let probe = match probe(&ffprobe, path) {
        Ok(probe) => probe,
        Err(error) => {
            let (name, _) = sniffed.ok_or_else(|| {
                ConvertError::Invalid(format!("not a readable audio/video file: {error}"))
            })?;
            return Ok(basic(
                name,
                bytes.len() as u64,
                given_path,
                &format!("_ffprobe could not read this file: {error}_"),
            ));
        }
    };
    Ok(render_probe(
        &probe,
        path,
        given_path,
        sniffed.map(|(n, _)| n),
        options,
    ))
}

fn probe(ffprobe: &Path, path: &Path) -> Result<Value, String> {
    let args: [&std::ffi::OsStr; 9] = [
        "-v".as_ref(),
        "quiet".as_ref(),
        "-print_format".as_ref(),
        "json".as_ref(),
        "-show_format".as_ref(),
        "-show_streams".as_ref(),
        "-show_chapters".as_ref(),
        "--".as_ref(),
        path.as_os_str(),
    ];
    let output = tool::run(ffprobe, args, PROBE_TIMEOUT)?;
    if !output.success {
        return Err("ffprobe exited with an error".into());
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("ffprobe returned unreadable JSON: {e}"))?;
    let has_streams = value
        .get("streams")
        .and_then(Value::as_array)
        .is_some_and(|s| !s.is_empty());
    if has_streams {
        Ok(value)
    } else {
        Err("no audio or video streams found".into())
    }
}

fn basic(container: &str, size: u64, path: Option<&Path>, note: &str) -> Converted {
    let facts = vec![
        ("container".to_string(), container.to_string()),
        ("file size".to_string(), crate::image::human_bytes(size)),
    ];
    let mut markdown = facts_table(&facts);
    markdown.push_str("\n\n");
    markdown.push_str(note);
    let mut sections = vec![Section {
        label: "media".into(),
        markdown,
    }];
    sections.extend(sidecar_sections(path));
    Converted {
        outline: Vec::new(),
        format: "video".into(),
        title: None,
        sections,
        metadata: facts,
    }
}

fn render_probe(
    probe: &Value,
    path: &Path,
    given_path: Option<&Path>,
    sniffed: Option<&str>,
    options: &Options,
) -> Converted {
    let format = probe.get("format").cloned().unwrap_or(Value::Null);
    let tag = |key: &str| tag_value(format.get("tags"), key);
    let streams: Vec<&Value> = probe
        .get("streams")
        .and_then(Value::as_array)
        .map(|s| s.iter().collect())
        .unwrap_or_default();
    fn kind_of(stream: &Value) -> &str {
        stream
            .get("codec_type")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
    let is_cover = |s: &&Value| {
        s.get("disposition")
            .and_then(|d| d.get("attached_pic"))
            .and_then(Value::as_i64)
            == Some(1)
    };
    let has_video = streams
        .iter()
        .any(|s| kind_of(s) == "video" && !is_cover(s));
    let has_audio = streams.iter().any(|s| kind_of(s) == "audio");

    let mut facts: Vec<(String, String)> = Vec::new();
    let container_name = format
        .get("format_long_name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| sniffed.map(str::to_string))
        .unwrap_or_else(|| "unknown".into());
    facts.push(("container".into(), container_name));
    facts.push((
        "kind".into(),
        if has_video { "video" } else { "audio" }.into(),
    ));
    if let Some(duration) = number(format.get("duration")) {
        facts.push(("duration".into(), timestamp(duration, duration >= 3600.0)));
    }
    if let Some(size) = number(format.get("size")) {
        facts.push(("file size".into(), crate::image::human_bytes(size as u64)));
    }
    if let Some(rate) = number(format.get("bit_rate")) {
        facts.push(("bitrate".into(), format!("{:.0} kb/s", rate / 1000.0)));
    }
    for key in ["artist", "album", "date", "genre", "comment"] {
        if let Some(value) = tag(key) {
            facts.push((key.into(), value.chars().take(200).collect()));
        }
    }

    let mut markdown = facts_table(&facts);
    let stream_lines: Vec<String> = streams
        .iter()
        .filter_map(|s| describe_stream(s, is_cover(s)))
        .collect();
    if !stream_lines.is_empty() {
        markdown.push_str("\n\n## Streams\n\n");
        markdown.push_str(&stream_lines.join("\n"));
    }

    let chapters = probe
        .get("chapters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let long = number(format.get("duration")).is_some_and(|d| d >= 3600.0);
    let chapter_lines: Vec<String> = chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            let start = number(chapter.get("start_time")).unwrap_or(0.0);
            let title = tag_value(chapter.get("tags"), "title")
                .unwrap_or_else(|| format!("Chapter {}", index + 1));
            format!("- {} {}", timestamp(start, long), title)
        })
        .collect();
    if !chapter_lines.is_empty() {
        markdown.push_str("\n\n## Chapters\n\n");
        markdown.push_str(&chapter_lines.join("\n"));
    }

    let mut extra_sections = Vec::new();
    let mut notes = Vec::new();

    // Embedded subtitles: the first text subtitle stream.
    let subtitle_streams: Vec<&&Value> = streams
        .iter()
        .filter(|s| kind_of(s) == "subtitle")
        .collect();
    if let Some(first) = subtitle_streams.first() {
        let codec = first
            .get("codec_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if matches!(
            codec,
            "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub"
        ) {
            notes.push(format!(
                "_Embedded subtitles are images ({codec}); they need OCR and are not extracted._"
            ));
        } else {
            match extract_subtitles(path) {
                Ok(text) if !text.is_empty() => extra_sections.push(Section {
                    label: "subtitles".into(),
                    markdown: text,
                }),
                Ok(_) => {}
                Err(error) => {
                    notes.push(format!("_Embedded subtitles were not extracted: {error}_"))
                }
            }
        }
    }
    extra_sections.extend(sidecar_sections(given_path));

    if has_audio {
        if options.transcript {
            match transcribe(path, options.download_asr_model) {
                Ok(text) if !text.is_empty() => extra_sections.push(Section {
                    label: "transcript".into(),
                    markdown: text,
                }),
                Ok(_) => notes.push("_Transcript: no speech was recognised._".into()),
                Err(error) => {
                    // First line is the summary; any further lines are how-to bullets.
                    let (head, rest) = error.split_once('\n').unwrap_or((&error, ""));
                    let mut note = format!("_Transcript unavailable: {head}_");
                    if !rest.is_empty() {
                        note.push('\n');
                        note.push_str(rest);
                    }
                    notes.push(note);
                }
            }
        } else {
            notes.push(
                "_Pass `transcript: true` for a speech transcript (local Qwen3-ASR; `anymd doctor` shows what is installed)._"
                    .into(),
            );
        }
    }
    if !notes.is_empty() {
        markdown.push_str("\n\n");
        markdown.push_str(&notes.join("\n"));
    }

    let mut sections = vec![Section {
        label: "media".into(),
        markdown,
    }];
    sections.extend(extra_sections);
    Converted {
        outline: Vec::new(),
        format: "video".into(),
        title: tag("title"),
        sections,
        metadata: facts,
    }
}

fn describe_stream(stream: &Value, cover: bool) -> Option<String> {
    let kind = stream.get("codec_type").and_then(Value::as_str)?;
    let codec = stream
        .get("codec_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut parts = vec![codec.to_string()];
    match kind {
        "video" => {
            if let (Some(w), Some(h)) = (
                stream.get("width").and_then(Value::as_u64),
                stream.get("height").and_then(Value::as_u64),
            ) {
                parts.push(format!("{w}×{h}"));
            }
            if !cover {
                let fps = ["avg_frame_rate", "r_frame_rate"]
                    .iter()
                    .filter_map(|k| stream.get(*k).and_then(Value::as_str).and_then(frame_rate))
                    .next();
                if let Some(fps) = fps {
                    parts.push(format!("{} fps", trim_float(fps)));
                }
            }
        }
        "audio" => {
            if let Some(rate) = number(stream.get("sample_rate")) {
                parts.push(format!("{} kHz", trim_float(rate / 1000.0)));
            }
            let layout = stream.get("channel_layout").and_then(Value::as_str);
            match (stream.get("channels").and_then(Value::as_u64), layout) {
                (Some(ch), Some(layout)) => parts.push(format!("{layout} ({ch} ch)")),
                (Some(ch), None) => parts.push(format!("{ch} ch")),
                _ => {}
            }
        }
        _ => {}
    }
    if let Some(lang) = tag_value(stream.get("tags"), "language").filter(|l| l != "und") {
        parts.push(format!("lang {lang}"));
    }
    if let Some(title) = tag_value(stream.get("tags"), "title") {
        parts.push(format!("\"{title}\""));
    }
    let label = match kind {
        "video" if cover => "Cover art",
        "video" => "Video",
        "audio" => "Audio",
        "subtitle" => "Subtitle",
        "data" => return None,
        "attachment" => "Attachment",
        _ => "Stream",
    };
    Some(format!("- {label}: {}", parts.join(", ")))
}

fn extract_subtitles(path: &Path) -> Result<String, String> {
    let ffmpeg = tool::find("ffmpeg").ok_or("needs ffmpeg installed")?;
    let args: [&std::ffi::OsStr; 14] = [
        "-nostdin".as_ref(),
        "-hide_banner".as_ref(),
        "-loglevel".as_ref(),
        "error".as_ref(),
        "-i".as_ref(),
        path.as_os_str(),
        "-map".as_ref(),
        "0:s:0".as_ref(),
        "-c:s".as_ref(),
        "srt".as_ref(),
        "-f".as_ref(),
        "srt".as_ref(),
        "-y".as_ref(),
        "-".as_ref(),
    ];
    let output = tool::run(&ffmpeg, args, SUBTITLE_TIMEOUT)?;
    if !output.success {
        return Err("ffmpeg could not read the subtitle stream".into());
    }
    Ok(subtitles_to_markdown(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// `movie.srt`, `movie.vtt`, `movie.en.srt`… next to the media file.
fn sidecar_sections(path: Option<&Path>) -> Vec<Section> {
    let Some(path) = path else { return Vec::new() };
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) else {
        return Vec::new();
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut sidecars: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            let lower = name.to_ascii_lowercase();
            (lower.ends_with(".srt") || lower.ends_with(".vtt"))
                && (name.starts_with(&format!("{stem}.")))
                && p.metadata()
                    .is_ok_and(|m| m.is_file() && m.len() <= MAX_SIDECAR_BYTES)
        })
        .collect();
    sidecars.sort();
    sidecars
        .into_iter()
        .take(4)
        .filter_map(|p| {
            let text = std::fs::read(&p).ok()?;
            let markdown = subtitles_to_markdown(&String::from_utf8_lossy(&text));
            let name = p.file_name()?.to_string_lossy().into_owned();
            (!markdown.is_empty()).then(|| Section {
                label: format!("subtitles ({name})"),
                markdown,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Transcript (bundled transcribe-cpp / Qwen3-ASR)

#[cfg(not(feature = "native"))]
fn transcribe(_path: &Path, _download: bool) -> Result<String, String> {
    Err("transcripts need the native anymd build".into())
}

#[derive(Debug, PartialEq)]
struct TranscriptCue {
    start_ms: u64,
    end_ms: u64,
    text: String,
    word: bool,
}

#[cfg(feature = "native")]
fn transcribe(path: &Path, download: bool) -> Result<String, String> {
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
) -> Result<Vec<TranscriptCue>, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let reference = dir.path().join("reference.txt");
    std::fs::write(&reference, text).map_err(|e| e.to_string())?;
    let output = tool::run(
        binary,
        [
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
        ],
        timeout,
    )?;
    if !output.success {
        return Err("ForcedAligner failed; retaining segment timestamps".into());
    }
    let payload = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    parse_alignment(&payload, text, duration_ms)
}

fn parse_alignment(
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

fn render_transcript(cues: &[TranscriptCue]) -> String {
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

// ---------------------------------------------------------------------------
// Subtitles

/// SRT or WebVTT text → one `[mm:ss] text` line per cue, markup stripped and
/// rolling-caption repeats removed. Non-subtitle text comes back trimmed.
pub fn subtitles_to_markdown(text: &str) -> String {
    let text = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut cues: Vec<(u64, String)> = Vec::new();
    let mut previous: Vec<String> = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(timing_index) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some(start) = lines[timing_index]
            .split("-->")
            .next()
            .and_then(parse_timestamp)
        else {
            continue;
        };
        let cue_lines: Vec<String> = lines[timing_index + 1..]
            .iter()
            .map(|l| clean_cue_line(l))
            .filter(|l| !l.is_empty())
            .collect();
        // Rolling captions repeat the previous cue's lines; keep only what is new.
        let fresh: Vec<String> = cue_lines
            .iter()
            .filter(|l| !previous.contains(l))
            .cloned()
            .collect();
        if !cue_lines.is_empty() {
            previous = cue_lines;
        }
        if fresh.is_empty() {
            continue;
        }
        cues.push((start, fresh.join(" ")));
    }
    if cues.is_empty() {
        return text.trim().to_string();
    }
    render_cues(&cues)
}

fn render_cues(cues: &[(u64, String)]) -> String {
    let long = cues.iter().any(|(ms, _)| *ms >= 3_600_000);
    cues.iter()
        .map(|(ms, text)| format!("[{}] {text}", timestamp(*ms as f64 / 1000.0, long)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn clean_cue_line(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let tag: String = chars.by_ref().take_while(|c| *c != '>').collect();
                // WebVTT voice span: <v Speaker> → "Speaker: ".
                if tag.starts_with("v ") || tag.starts_with("v.") {
                    let speaker = tag.split_once(' ').map(|(_, s)| s.trim()).unwrap_or("");
                    if !speaker.is_empty() {
                        out.push_str(speaker);
                        out.push_str(": ");
                    }
                }
            }
            '{' if chars.peek() == Some(&'\\') => {
                // ASS override block such as {\an8}.
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `HH:MM:SS,mmm`, `HH:MM:SS.mmm`, or `MM:SS.mmm` → milliseconds.
fn parse_timestamp(value: &str) -> Option<u64> {
    let value = value.split_whitespace().next()?;
    let (clock, fraction) = match value.rsplit_once([',', '.']) {
        Some((clock, fraction))
            if fraction.chars().all(|c| c.is_ascii_digit()) && !fraction.is_empty() =>
        {
            (clock, fraction)
        }
        _ => (value, "0"),
    };
    let parts: Vec<u64> = clock
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let seconds = match parts.as_slice() {
        [h, m, s] => h.checked_mul(3600)?.checked_add(m * 60)?.checked_add(*s)?,
        [m, s] => m.checked_mul(60)?.checked_add(*s)?,
        _ => return None,
    };
    let digits: String = fraction.chars().take(3).collect();
    let millis = format!("{digits:0<3}").parse::<u64>().ok()?;
    seconds.checked_mul(1000)?.checked_add(millis)
}

// ---------------------------------------------------------------------------
// Helpers

fn facts_table(facts: &[(String, String)]) -> String {
    let mut rows = vec![vec!["Property".to_string(), "Value".to_string()]];
    for (key, value) in facts {
        let mut label = key.clone();
        if let Some(first) = label.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        rows.push(vec![label, value.clone()]);
    }
    crate::markdown_table(&rows).trim_end().to_string()
}

fn tag_value(tags: Option<&Value>, key: &str) -> Option<String> {
    let tags = tags?.as_object()?;
    tags.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, v)| v.as_str())
        .map(|v| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|v| !v.is_empty())
}

fn number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let n = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())?;
    n.is_finite().then_some(n)
}

fn frame_rate(value: &str) -> Option<f64> {
    let (num, den) = value.split_once('/')?;
    let (num, den): (f64, f64) = (num.parse().ok()?, den.parse().ok()?);
    (den > 0.0 && num > 0.0).then(|| num / den)
}

fn trim_float(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn timestamp(seconds: f64, hours: bool) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours || h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_containers() {
        assert!(sniff(b"\0\0\0\x20ftypisom\0\0\0\0"));
        assert!(!sniff(b"\0\0\0\x20ftypavif\0\0\0\0"));
        assert!(sniff(&[
            0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x82, 0x84, b'w', b'e', b'b', b'm'
        ]));
        assert_eq!(container(b"RIFF\0\0\0\0WAVEfmt ").map(|c| c.0), Some("WAV"));
        assert_eq!(container(b"RIFF\0\0\0\0AVI LIST").map(|c| c.0), Some("AVI"));
        assert!(sniff(b"ID3\x04\0\0\0\0"));
        assert!(sniff(&[0xFF, 0xFB, 0x90, 0x64]));
        assert!(sniff(b"fLaC\0\0\0\x22"));
        assert!(sniff(b"OggS\0\x02"));
        assert!(!sniff(b"plain text"));
        assert!(!sniff(&[0xFF, 0xD8, 0xFF, 0xE0]));
    }

    #[test]
    fn srt_to_timestamped_lines() {
        let srt = "\u{feff}1\r\n00:00:01,500 --> 00:00:03,000\r\n<i>Hello</i>\r\nthere\r\n\r\n2\r\n00:01:02,000 --> 00:01:04,000\r\n{\\an8}General &amp; Kenobi\r\n\r\n";
        assert_eq!(
            subtitles_to_markdown(srt),
            "[00:01] Hello there\n[01:02] General & Kenobi"
        );
    }

    #[test]
    fn vtt_voices_rolling_captions_and_hours() {
        let vtt = "WEBVTT\nKind: captions\n\nNOTE a comment\n\n00:00.000 --> 00:02.000\n<v Roger Bingham>We are here\n\n\
                   00:02.000 --> 00:04.000 align:start\nwe are here\nand now more\n\n\
                   00:04.000 --> 00:06.000\nand now more\n\n01:00:00.000 --> 01:00:01.000\n<c.yellow>late</c>";
        assert_eq!(
            subtitles_to_markdown(vtt),
            "[00:00:00] Roger Bingham: We are here\n[00:00:02] we are here and now more\n[01:00:00] late"
        );
    }

    #[test]
    fn alignment_preserves_text_and_validates_bounds() {
        let json = include_str!("../tests/fixtures/aligner/crispasr-0.8.38-words.json");
        let cues = parse_alignment(json, "Hello world", 1000).unwrap();
        assert_eq!(cues[0].start_ms, 100);
        assert!(render_transcript(&cues).contains("word (Qwen3-ForcedAligner)"));
        assert!(parse_alignment(json, "Hello world again", 1000).is_err());
        assert!(parse_alignment(json, "Hello world", 900).is_err());
        assert!(parse_alignment("[]", "Hello", 1000).is_err());
        assert!(
            parse_alignment(r#"[{"start":1.0,"end":0.5,"word":"Hello"}]"#, "Hello", 1000).is_err()
        );
        assert!(parse_alignment(
            r#"[{"start":0.1,"end":0.7,"word":"Hello"},{"start":0.6,"end":0.9,"word":"world"}]"#,
            "Hello world",
            1000
        )
        .is_err());
    }

    #[test]
    fn chunk_timestamps_remain_on_the_source_timeline() {
        let cues = vec![
            TranscriptCue {
                start_ms: 20_000,
                end_ms: 40_000,
                text: "second chunk".into(),
                word: false,
            },
            TranscriptCue {
                start_ms: 40_000,
                end_ms: 41_250,
                text: "last partial chunk".into(),
                word: false,
            },
        ];
        let rendered = render_transcript(&cues);
        assert!(rendered.contains("segment (20-second chunk boundaries, not word timing)"));
        assert!(rendered.contains("[00:00:20.000 --> 00:00:40.000] second chunk"));
        assert!(rendered.contains("[00:00:40.000 --> 00:00:41.250] last partial chunk"));
        assert!(render_transcript(&[]).is_empty());
    }

    #[test]
    fn timestamps_parse_and_reject_garbage() {
        assert_eq!(parse_timestamp("01:02:03,004"), Some(3_723_004));
        assert_eq!(parse_timestamp("02:03.5"), Some(123_500));
        assert_eq!(parse_timestamp("nope"), None);
        assert_eq!(parse_timestamp("99999999999999999999:00:00"), None);
        assert_eq!(subtitles_to_markdown("just text"), "just text");
    }

    #[test]
    fn unrecognized_bytes_are_invalid() {
        assert!(matches!(
            convert(b"definitely not media", &Options::default()),
            Err(ConvertError::Invalid(_))
        ));
    }

    /// End-to-end with real ffmpeg when present: a 1 s clip with a chapter and subtitles.
    #[test]
    fn probes_generated_clip_when_ffmpeg_exists() {
        let Some(ffmpeg) = tool::find("ffmpeg") else {
            return;
        };
        if tool::find("ffprobe").is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let srt = dir.path().join("in.srt");
        std::fs::write(&srt, "1\n00:00:00,100 --> 00:00:00,900\nHi there\n").unwrap();
        let meta = dir.path().join("meta.txt");
        std::fs::write(&meta, ";FFMETADATA1\ntitle=Clip\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=Opening\n").unwrap();
        let clip = dir.path().join("clip.mkv");
        let sidecar = dir.path().join("clip.en.srt");
        std::fs::write(&sidecar, "1\n00:00:00,000 --> 00:00:01,000\nSidecar line\n").unwrap();
        let args: Vec<std::ffi::OsString> = [
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=d=1:s=64x48:r=10",
            "-f",
            "lavfi",
            "-i",
            "sine=d=1",
            "-i",
        ]
        .iter()
        .map(Into::into)
        .chain([srt.into_os_string(), "-i".into(), meta.into_os_string()])
        .chain(
            [
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:s",
                "-map_metadata",
                "3",
                "-map_chapters",
                "3",
                "-c:v",
                "mpeg4",
                "-c:a",
                "flac",
                "-c:s",
                "srt",
                "-y",
            ]
            .iter()
            .map(Into::into),
        )
        .chain([clip.clone().into_os_string()])
        .collect();
        let made = tool::run(&ffmpeg, &args, Duration::from_secs(60)).unwrap();
        if !made.success {
            return; // ffmpeg build without these codecs; nothing to assert.
        }
        let bytes = std::fs::read(&clip).unwrap();
        let converted = convert(
            &bytes,
            &Options {
                path: Some(clip.clone()),
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(converted.title.as_deref(), Some("Clip"));
        let media = &converted.sections[0].markdown;
        assert!(media.contains("|Kind|video|"), "{media}");
        assert!(media.contains("- Video: mpeg4, 64×48, 10 fps"), "{media}");
        assert!(media.contains("- Audio: flac, 44.1 kHz"), "{media}");
        assert!(media.contains("## Chapters\n\n- 00:00 Opening"), "{media}");
        assert!(media.contains("`transcript: true`"), "{media}");
        let labels: Vec<&str> = converted
            .sections
            .iter()
            .map(|s| s.label.as_str())
            .collect();
        assert_eq!(labels, ["media", "subtitles", "subtitles (clip.en.srt)"]);
        assert_eq!(converted.sections[1].markdown, "[00:00] Hi there");
        assert_eq!(converted.sections[2].markdown, "[00:00] Sidecar line");

        // Bytes only (no path): probed through a temp file.
        let converted = convert(&bytes, &Options::default()).unwrap();
        assert_eq!(converted.sections.len(), 2);

        // The transcript path downloads weights only when explicitly requested;
        // ordinary media conversion above remains offline and model-free.
    }
}
