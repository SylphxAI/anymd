//! ffprobe invocation and rendering of media facts, streams, and chapters.

use std::path::Path;

use serde_json::Value;

use super::subtitles::{extract_subtitles, sidecar_sections};
use super::transcribe::transcribe;
use super::PROBE_TIMEOUT;
use crate::{tool, Converted, Options, Section};

pub(super) fn probe(ffprobe: &Path, path: &Path) -> Result<Value, String> {
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

pub(super) fn basic(
    container: &str,
    size: u64,
    path: Option<&Path>,
    source_access: &dyn crate::SourceAccess,
    note: &str,
) -> Converted {
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
    sections.extend(sidecar_sections(path, source_access));
    Converted {
        outline: Vec::new(),
        format: "video".into(),
        title: None,
        sections,
        metadata: facts,
    }
}

pub(super) fn render_probe(
    probe: &Value,
    path: &Path,
    given_path: Option<&Path>,
    sniffed: Option<&str>,
    options: &Options,
    source_access: &dyn crate::SourceAccess,
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
    extra_sections.extend(sidecar_sections(given_path, source_access));

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
