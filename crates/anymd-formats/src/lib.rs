//! anymd format converters: every non-PDF input becomes Markdown sections.
//!
//! Each module exposes `convert(bytes, &Options) -> Result<Converted, ConvertError>`.
//! A `Section` is the unit a reader paginates over and cites: a slide, a sheet,
//! an EPUB chapter, or a whole document when the format has no natural pages.

#![cfg_attr(not(feature = "native"), allow(dead_code))]

pub mod csv;
pub mod docx;
pub mod epub;
pub mod html;
pub mod image;
pub mod pptx;
#[cfg(feature = "native")]
mod tool;
/// Without `native` (WebAssembly) no helper binary exists: every lookup misses,
/// so OCR, ffprobe, and whisper paths fall back to their no-tool output.
#[cfg(not(feature = "native"))]
mod tool {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    pub(crate) struct ToolOutput {
        pub success: bool,
        pub stdout: Vec<u8>,
        pub stderr: Vec<u8>,
    }

    pub(crate) struct TempFile(PathBuf);

    impl TempFile {
        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    pub(crate) fn find(_name: &str) -> Option<PathBuf> {
        None
    }

    pub(crate) fn run<I, S>(_program: &Path, _args: I, _timeout: Duration) -> Result<ToolOutput, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        Err("local tools are not available in this build".into())
    }

    pub(crate) fn temp_file(_bytes: &[u8], _suffix: &str) -> Result<TempFile, String> {
        Err("temp files are not available in this build".into())
    }
}
pub mod video;
pub mod whisper;
pub mod xlsx;

mod ooxml;

use std::path::Path;

/// One citable unit of a converted document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Section {
    /// Human label used in page markers, e.g. "slide 3", "sheet Revenue", "chapter 2".
    pub label: String,
    /// Clean Markdown for this unit (no leading marker; the reader adds it).
    pub markdown: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Converted {
    /// Short format name: "docx", "pptx", "xlsx", "csv", "epub", "html", "image", "video".
    pub format: String,
    pub title: Option<String>,
    pub sections: Vec<Section>,
    /// Small key/value facts worth a header line (author, dimensions, duration...).
    pub metadata: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Base URL for resolving relative links (HTML fetched from a URL).
    pub base_url: Option<String>,
    /// Opt-in OCR for images (runs a local `tesseract` binary when present).
    pub ocr: bool,
    /// Opt-in transcript for audio/video (runs a local whisper.cpp binary when present).
    pub transcript: bool,
    /// With `transcript`: download the configured ggml whisper model into the
    /// anymd cache when none is installed (see [`whisper`]).
    pub download_whisper_model: bool,
    /// Source path when the input came from disk (video/ffprobe needs a path).
    pub path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// The bytes are not a valid instance of the format.
    Invalid(String),
    /// The format needs a tool or feature that is not available here.
    Unsupported(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) | Self::Unsupported(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ConvertError {}

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
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "tiff" | "tif" | "bmp") => Some(Format::Image),
        Some("mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" | "mp3" | "wav" | "m4a" | "flac" | "ogg") => {
            Some(Format::Video)
        }
        _ => {
            let text_head = String::from_utf8_lossy(&head[..head.len().min(512)]).to_ascii_lowercase();
            let trimmed = text_head.trim_start();
            if trimmed.starts_with("<!doctype html") || trimmed.starts_with("<html") {
                Some(Format::Html)
            } else if std::str::from_utf8(head).is_ok() || head.is_empty() {
                Some(Format::Text)
            } else {
                None
            }
        }
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

/// Convert a non-PDF input. PDF is handled by the PDF engine, not this crate.
pub fn convert(format: Format, bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    match format {
        Format::Docx => docx::convert(bytes, options),
        Format::Pptx => pptx::convert(bytes, options),
        Format::Xlsx => xlsx::convert(bytes, options),
        Format::Csv => csv::convert(bytes, b',', options),
        Format::Tsv => csv::convert(bytes, b'\t', options),
        Format::Epub => epub::convert(bytes, options),
        Format::Html => html::convert(bytes, options),
        Format::Image => image::convert(bytes, options),
        Format::Video => video::convert(bytes, options),
        Format::Text => Ok(Converted {
            format: "text".into(),
            title: None,
            sections: vec![Section {
                label: "document".into(),
                markdown: String::from_utf8_lossy(bytes).into_owned(),
            }],
            metadata: Vec::new(),
        }),
        Format::Subtitles => Ok(Converted {
            format: "subtitles".into(),
            title: None,
            sections: vec![Section {
                label: "subtitles".into(),
                markdown: video::subtitles_to_markdown(&String::from_utf8_lossy(bytes)),
            }],
            metadata: Vec::new(),
        }),
        Format::Pdf => Err(ConvertError::Unsupported(
            "PDF is converted by the PDF engine, not anymd-formats".into(),
        )),
    }
}

/// Escape a table cell for a Markdown pipe table.
pub fn table_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace("\r\n", " ")
        .replace(['\n', '\r'], " ")
        .trim()
        .to_string()
}

/// Render rows as a Markdown pipe table; the first row is the header.
pub fn markdown_table(rows: &[Vec<String>]) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        // Compact pipe tables: padding spaces cost ~20% more tokens.
        out.push('|');
        for column in 0..width {
            out.push_str(&table_cell(row.get(column).map(String::as_str).unwrap_or("")));
            out.push('|');
        }
        out.push('\n');
        if index == 0 {
            out.push('|');
            for _ in 0..width {
                out.push_str("-|");
            }
            out.push('\n');
        }
    }
    out
}
