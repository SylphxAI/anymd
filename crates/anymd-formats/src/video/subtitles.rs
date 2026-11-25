//! Embedded and sidecar subtitles, with shared SRT/WebVTT cue parsing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{tool, Section};

const SUBTITLE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_SIDECAR_BYTES: u64 = 10 * 1024 * 1024;

pub(super) fn extract_subtitles(path: &Path) -> Result<String, String> {
    let ffmpeg = tool::find("ffmpeg").ok_or("needs ffmpeg installed")?;
    let args: [&std::ffi::OsStr; 15] = [
        "-nostdin".as_ref(),
        "-hide_banner".as_ref(),
        "-loglevel".as_ref(),
        "error".as_ref(),
        "-copyts".as_ref(),
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
pub(super) fn sidecar_sections(
    path: Option<&Path>,
    source_access: &dyn crate::SourceAccess,
) -> Vec<Section> {
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
    let mut sidecars: Vec<(PathBuf, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            let lower = name.to_ascii_lowercase();
            (lower.ends_with(".srt") || lower.ends_with(".vtt"))
                && (name.starts_with(&format!("{stem}.")))
        })
        .filter_map(|path| {
            // Keep the sibling name for provenance, but open only the admitted
            // path. A refusal must never fall back to the discovered path.
            let admitted = source_access.admit_path(&path).ok()?;
            admitted
                .metadata()
                .is_ok_and(|m| m.is_file() && m.len() <= MAX_SIDECAR_BYTES)
                .then_some((path, admitted))
        })
        .collect();
    sidecars.sort();
    sidecars
        .into_iter()
        .take(4)
        .filter_map(|(p, admitted)| {
            use std::io::Read;
            let file = std::fs::File::open(admitted).ok()?;
            let mut text = Vec::new();
            file.take(MAX_SIDECAR_BYTES + 1)
                .read_to_end(&mut text)
                .ok()?;
            if text.len() as u64 > MAX_SIDECAR_BYTES {
                return None;
            }
            let markdown = subtitles_to_markdown(&String::from_utf8_lossy(&text));
            let name = p.file_name()?.to_string_lossy().into_owned();
            (!markdown.is_empty()).then(|| Section {
                label: format!("subtitles ({name})"),
                markdown,
            })
        })
        .collect()
}

/// SRT or WebVTT text → one timestamped line per nonempty cue, markup stripped.
/// Cues are source evidence: repeated or overlapping text is not deduplicated.
/// Non-subtitle text comes back trimmed.
pub fn subtitles_to_markdown(text: &str) -> String {
    let text = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut cues: Vec<(u64, String)> = Vec::new();
    let mut timed = false;
    let webvtt = text.starts_with("WEBVTT");
    let lines: Vec<&str> = text.lines().collect();
    for lines in lines.split(|line| line.trim().is_empty()) {
        // These WebVTT blocks are metadata, even if they contain a timing line.
        if webvtt
            && lines.first().is_some_and(|line| {
                *line == "NOTE"
                    || line.starts_with("NOTE ")
                    || line.starts_with("NOTE\t")
                    || *line == "STYLE"
                    || *line == "REGION"
                    || line.starts_with("WEBVTT")
            })
        {
            continue;
        }
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
        timed = true;
        let cue_lines: Vec<String> = lines[timing_index + 1..]
            .iter()
            .map(|l| clean_cue_line(l))
            .filter(|l| !l.is_empty())
            .collect();
        if !cue_lines.is_empty() {
            cues.push((start, cue_lines.join(" ")));
        }
    }
    if !timed {
        return text.trim().to_string();
    }
    render_cues(&cues)
}

fn render_cues(cues: &[(u64, String)]) -> String {
    let long = cues.iter().any(|(ms, _)| *ms >= 3_600_000);
    cues.iter()
        .map(|(ms, text)| {
            // Keep source milliseconds in integer arithmetic, not a float
            // round-trip or the whole-second media-metadata formatter.
            let seconds = ms / 1000;
            let clock = if long {
                format!(
                    "{:02}:{:02}:{:02}",
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60
                )
            } else {
                format!("{:02}:{:02}", seconds / 60, seconds % 60)
            };
            if ms % 1000 == 0 {
                format!("[{clock}] {text}")
            } else {
                format!("[{clock}.{:03}] {text}", ms % 1000)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn clean_cue_line(line: &str) -> String {
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
pub(super) fn parse_timestamp(value: &str) -> Option<u64> {
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
        [h, m, s] if *m < 60 && *s < 60 => h
            .checked_mul(3600)?
            .checked_add(m.checked_mul(60)?)?
            .checked_add(*s)?,
        [m, s] if *s < 60 => m.checked_mul(60)?.checked_add(*s)?,
        _ => return None,
    };
    let digits: String = fraction.chars().take(3).collect();
    let millis = format!("{digits:0<3}").parse::<u64>().ok()?;
    seconds.checked_mul(1000)?.checked_add(millis)
}
