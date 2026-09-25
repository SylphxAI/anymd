//! Audio/video → Markdown: container facts, streams, chapters, subtitles, and an
//! opt-in transcript. Ported from cue `video-reader-core` (ffprobe, subtitle
//! extraction, whisper.cpp orchestration), reduced to what reads well as text.
//!
//! Everything degrades: without ffprobe the output is the sniffed container plus
//! an install hint; without whisper.cpp the transcript is a how-to line.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::{tool, ConvertError, Converted, Options, Section};

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const SUBTITLE_TIMEOUT: Duration = Duration::from_secs(60);
const AUDIO_EXTRACT_TIMEOUT: Duration = Duration::from_secs(600);
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(1800);
const MAX_SIDECAR_BYTES: u64 = 10 * 1024 * 1024;
const WHISPER_ADAPTERS: &[&str] = &["whisper-cli", "whisper-cpp"];
const MODEL_ENV: &str = "ANYMD_WHISPER_MODEL";

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
            "_Install ffmpeg (`ffprobe`) to report duration, streams, chapters, and subtitles._",
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
            match transcribe(path) {
                Ok(text) if !text.is_empty() => extra_sections.push(Section {
                    label: "transcript".into(),
                    markdown: text,
                }),
                Ok(_) => notes.push("_Transcript: no speech was recognised._".into()),
                Err(error) => notes.push(format!("_Transcript unavailable: {error}_")),
            }
        } else {
            notes.push(
                "_Pass `transcript: true` for a speech transcript (local whisper.cpp; see ANYMD_WHISPER_MODEL)._"
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
// Transcript (whisper.cpp)

fn transcribe(path: &Path) -> Result<String, String> {
    let adapter = WHISPER_ADAPTERS.iter().find_map(|name| tool::find(name));
    let model = std::env::var_os(MODEL_ENV)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    let ffmpeg = tool::find("ffmpeg");
    let (Some(adapter), Some(model), Some(ffmpeg)) = (adapter, model, ffmpeg) else {
        let mut missing = Vec::new();
        if WHISPER_ADAPTERS
            .iter()
            .all(|name| tool::find(name).is_none())
        {
            missing.push("a whisper.cpp binary (`whisper-cli`) on PATH".to_string());
        }
        if std::env::var_os(MODEL_ENV)
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .is_none()
        {
            missing.push(format!(
                "{MODEL_ENV} set to a ggml model file (e.g. ggml-base.en.bin)"
            ));
        }
        if tool::find("ffmpeg").is_none() {
            missing.push("ffmpeg".to_string());
        }
        return Err(format!("needs {}", missing.join(", ")));
    };
    let dir = tempfile::tempdir().map_err(|e| format!("temp dir: {e}"))?;
    let wav = dir.path().join("audio.wav");
    let extract: Vec<&std::ffi::OsStr> = vec![
        "-nostdin".as_ref(),
        "-hide_banner".as_ref(),
        "-loglevel".as_ref(),
        "error".as_ref(),
        "-y".as_ref(),
        "-i".as_ref(),
        path.as_os_str(),
        "-vn".as_ref(),
        "-ac".as_ref(),
        "1".as_ref(),
        "-ar".as_ref(),
        "16000".as_ref(),
        "-f".as_ref(),
        "wav".as_ref(),
        wav.as_os_str(),
    ];
    let output = tool::run(&ffmpeg, extract, AUDIO_EXTRACT_TIMEOUT)?;
    if !output.success {
        return Err("ffmpeg could not extract the audio track".into());
    }
    let prefix = dir.path().join("transcript");
    let args: Vec<&std::ffi::OsStr> = vec![
        "-m".as_ref(),
        model.as_os_str(),
        "-f".as_ref(),
        wav.as_os_str(),
        "-oj".as_ref(),
        "-of".as_ref(),
        prefix.as_os_str(),
    ];
    let output = tool::run(&adapter, args, TRANSCRIBE_TIMEOUT)?;
    if !output.success {
        return Err(format!(
            "whisper.cpp failed: {}",
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or("")
                .trim()
        ));
    }
    let json = std::fs::read_to_string(prefix.with_extension("json"))
        .map_err(|e| format!("whisper.cpp wrote no JSON output: {e}"))?;
    parse_whisper_json(&json)
}

/// whisper.cpp `-oj` output → timestamped lines.
fn parse_whisper_json(payload: &str) -> Result<String, String> {
    let root: Value =
        serde_json::from_str(payload).map_err(|e| format!("unreadable whisper JSON: {e}"))?;
    let entries = root
        .get("transcription")
        .or_else(|| root.get("segments"))
        .and_then(Value::as_array)
        .ok_or("whisper JSON has no transcription array")?;
    let mut cues = Vec::new();
    for entry in entries {
        let text = entry
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if text.is_empty() {
            continue;
        }
        let start_ms = entry
            .get("offsets")
            .and_then(|o| o.get("from"))
            .and_then(Value::as_u64)
            .or_else(|| {
                entry
                    .get("timestamps")
                    .and_then(|t| t.get("from"))
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp)
            })
            .or_else(|| {
                entry
                    .get("start")
                    .and_then(Value::as_f64)
                    .map(|s| (s * 1000.0) as u64)
            })
            .unwrap_or(0);
        cues.push((start_ms, text.to_string()));
    }
    Ok(render_cues(&cues))
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
    fn whisper_json_is_parsed() {
        let json = r#"{"transcription":[{"timestamps":{"from":"00:00:00,000","to":"00:00:02,000"},"offsets":{"from":0,"to":2000},"text":" Hello world."},{"offsets":{"from":65000,"to":66000},"text":"Bye."}]}"#;
        assert_eq!(
            parse_whisper_json(json).unwrap(),
            "[00:00] Hello world.\n[01:05] Bye."
        );
        assert!(parse_whisper_json("{}").is_err());
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
        assert!(media.contains("| Kind | video |"), "{media}");
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

        // Transcript requested without whisper: a how-to line, not an error.
        if tool::find("whisper-cli").is_none() && tool::find("whisper-cpp").is_none() {
            let converted = convert(
                &bytes,
                &Options {
                    transcript: true,
                    ..Options::default()
                },
            )
            .unwrap();
            assert!(
                converted.sections[0]
                    .markdown
                    .contains("Transcript unavailable: needs"),
                "{:?}",
                converted
            );
        }
    }
}
