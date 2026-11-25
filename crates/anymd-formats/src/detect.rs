//! Format identification from file signatures, extensions, and text prefixes.

use std::path::Path;

use crate::{encoding, image, video};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Docx,
    Pptx,
    Xlsx,
    Csv,
    Tsv,
    Epub,
    Html,
    Image,
    Video,
    /// Markdown, plain text, JSON, XML, source code: passed through as-is.
    Text,
    /// SubRip / WebVTT subtitles.
    Subtitles,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Pptx => "pptx",
            Self::Xlsx => "xlsx",
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Epub => "epub",
            Self::Html => "html",
            Self::Image => "image",
            Self::Video => "video",
            Self::Text => "text",
            Self::Subtitles => "subtitles",
        }
    }
}

/// Detect a format from magic bytes first, then the file extension.
pub fn detect(path: Option<&Path>, head: &[u8]) -> Option<Format> {
    if head.starts_with(b"%PDF-") {
        return Some(Format::Pdf);
    }
    let ext = path
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if head.starts_with(b"PK\x03\x04") {
        // OOXML and EPUB are zip containers; the extension (or mimetype entry) decides.
        return match ext.as_deref() {
            Some("docx" | "docm" | "dotx") => Some(Format::Docx),
            Some("pptx" | "pptm" | "ppsx") => Some(Format::Pptx),
            Some("xlsx" | "xlsm" | "xltx") => Some(Format::Xlsx),
            Some("epub") => Some(Format::Epub),
            _ => sniff_zip(head),
        };
    }
    if image::sniff(head) {
        return Some(Format::Image);
    }
    // FF FE is both a UTF-16LE BOM and a possible MPEG sync prefix. Resolve
    // BOM-marked text before the loose media probe, without overriding known
    // binary signatures or explicit binary formats (e.g. a real Layer I MP3).
    if encoding::has_utf16_bom(head) && !mpeg_layer_one_frames(head) {
        let bom_format = match ext.as_deref() {
            Some("html" | "htm" | "xhtml") => Some(Format::Html),
            Some("csv") => Some(Format::Csv),
            Some("tsv" | "tab") => Some(Format::Tsv),
            Some("txt" | "md" | "markdown") => Some(Format::Text),
            Some(
                "pdf" | "docx" | "pptx" | "xlsx" | "epub" | "srt" | "vtt" | "png" | "jpg" | "jpeg"
                | "gif" | "webp" | "tiff" | "tif" | "bmp" | "mp4" | "mov" | "mkv" | "webm" | "avi"
                | "m4v" | "mp3" | "aac" | "wav" | "m4a" | "flac" | "ogg",
            ) => None,
            _ => {
                let prefix = encoding::prefix(head);
                (!prefix.chars().any(encoding::binary_control)).then(|| text_format(&prefix))
            }
        };
        if bom_format.is_some() {
            return bom_format;
        }
    }
    if video::sniff(head) {
        return Some(Format::Video);
    }
    match ext.as_deref() {
        Some("pdf") => Some(Format::Pdf),
        Some("docx") => Some(Format::Docx),
        Some("pptx") => Some(Format::Pptx),
        Some("xlsx" | "xlsm" | "xls" | "ods") => Some(Format::Xlsx),
        Some("csv") => Some(Format::Csv),
        Some("srt" | "vtt") => Some(Format::Subtitles),
        Some("tsv" | "tab") => Some(Format::Tsv),
        Some("epub") => Some(Format::Epub),
        Some("html" | "htm" | "xhtml") => Some(Format::Html),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "tiff" | "tif" | "bmp") => {
            Some(Format::Image)
        }
        Some(
            "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" | "mp3" | "wav" | "m4a" | "flac" | "ogg",
        ) => Some(Format::Video),
        _ => {
            let prefix = encoding::prefix(head);
            if text_format(&prefix) == Format::Html {
                Some(Format::Html)
            } else if std::str::from_utf8(head).is_ok()
                || (head.len() == 8192
                    && std::str::from_utf8(head)
                        .is_err_and(|error| error.error_len().is_none() && error.valid_up_to() > 0))
            {
                Some(Format::Text)
            } else {
                None
            }
        }
    }
}

/// FF FE can also start CRC-protected MPEG-1 Layer I audio. A single loose
/// sync match is not enough to override a BOM; require the next complete frame
/// header at the length declared by the first. Other media signatures remain
/// handled by the media converter, which is also used for explicit media paths.
fn mpeg_layer_one_frames(head: &[u8]) -> bool {
    if head.len() < 4 || !head.starts_with(&[0xFF, 0xFE]) {
        return false;
    }
    let bitrate = [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 0,
    ][usize::from(head[2] >> 4)];
    let rate_index = (head[2] >> 2) & 3;
    if bitrate == 0 || rate_index == 3 {
        return false;
    }
    let sample_rate = [44100, 48000, 32000][usize::from(rate_index)];
    let padding = usize::from((head[2] >> 1) & 1);
    let next = (12 * bitrate * 1000 / sample_rate + padding) * 4;
    head.get(next..next + 4).is_some_and(|frame| {
        frame[0] == 0xFF
            && frame[1] & 0xFE == 0xFE
            && (frame[2] >> 2) & 3 == rate_index
            && !matches!(frame[2] >> 4, 0 | 15)
    })
}

fn text_format(prefix: &str) -> Format {
    let lower = prefix.trim_start().to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        Format::Html
    } else {
        Format::Text
    }
}

fn sniff_zip(head: &[u8]) -> Option<Format> {
    let window = &head[..head.len().min(4096)];
    let contains = |needle: &[u8]| window.windows(needle.len()).any(|w| w == needle);
    if contains(b"application/epub+zip") {
        Some(Format::Epub)
    } else if contains(b"word/") {
        Some(Format::Docx)
    } else if contains(b"ppt/") {
        Some(Format::Pptx)
    } else if contains(b"xl/") {
        Some(Format::Xlsx)
    } else {
        None
    }
}
