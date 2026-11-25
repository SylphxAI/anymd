//! Audio/video → Markdown: container facts, streams, chapters, subtitles, and an
//! opt-in transcript. Ported from cue `video-reader-core` (ffprobe, subtitle
//! extraction, local ASR orchestration), reduced to what reads well as text.
//!
//! Everything degrades: without ffprobe the output is the sniffed container plus
//! an install hint; without ffmpeg the transcript is a how-to line.

use std::path::Path;
use std::time::Duration;

use crate::{tool, ConvertError, Converted, Options};

mod probe;
mod subtitles;
mod transcribe;

/// Bounded, opt-in video evidence; legacy conversion is unchanged.
#[cfg(feature = "native")]
pub mod timeline;

#[cfg(test)]
mod tests;

use probe::{basic, probe, render_probe};
#[cfg(feature = "native")]
use subtitles::{clean_cue_line, parse_timestamp};

pub use subtitles::subtitles_to_markdown;
#[cfg(feature = "native")]
pub use transcribe::transcript_window;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

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
    convert_with_source_access(bytes, options, &crate::UnrestrictedSourceAccess)
}

/// Convert media while admitting every discovered subtitle through the caller.
pub fn convert_with_source_access(
    bytes: &[u8],
    options: &Options,
    source_access: &dyn crate::SourceAccess,
) -> Result<Converted, ConvertError> {
    let sniffed = container(bytes);
    let given_path = options.path.as_deref().filter(|p| p.is_file());

    let Some(ffprobe) = tool::find("ffprobe") else {
        let (name, _) = sniffed
            .ok_or_else(|| ConvertError::Invalid("not a recognized audio/video file".into()))?;
        return Ok(basic(
            name,
            bytes.len() as u64,
            given_path,
            source_access,
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
                source_access,
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
        source_access,
    ))
}
