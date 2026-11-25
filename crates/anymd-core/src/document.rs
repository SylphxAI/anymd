//! Convert document bytes to Markdown units without opening a source path.

use std::{fmt, path::Path};

use anymd_formats::{ConvertError, Format, Options, Section};
use anymd_pdf::{LayoutError, MarkdownPage, PdfDocument};

pub use anymd_formats::Revisions;

use crate::outline::{front_matter, noun_for_units, title_from_units, DocumentInfo, Outline, Unit};

/// PDF pages converted per pass, shared with the CLI reader so heading levels match.
pub const PAGE_CHUNK: usize = 12;

#[derive(Debug, Clone, Default)]
pub struct OpenBytesOptions {
    /// A format hint only; no file is opened at this path.
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub revisions: anymd_formats::Revisions,
}

#[derive(Debug)]
pub enum OpenError {
    UnrecognizedFormat,
    PdfParse(LayoutError),
    Conversion {
        format: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnrecognizedFormat => {
                f.write_str("Unrecognized file type (binary data with no known signature).")
            }
            Self::PdfParse(error) => write!(f, "Could not parse PDF: {error}"),
            Self::Conversion { format, source } => {
                write!(f, "Could not convert {format}: {source}")
            }
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnrecognizedFormat => None,
            Self::PdfParse(error) => Some(error),
            Self::Conversion { source, .. } => Some(source.as_ref()),
        }
    }
}

#[derive(Debug)]
pub struct OpenedDocument {
    pub format: &'static str,
    pub title: Option<String>,
    pub unit_noun: &'static str,
    pub metadata: Vec<(String, String)>,
    pub units: Vec<Unit>,
    /// Native outline entries: zero-based depth, title, and 1-based unit number (if known).
    pub native_outline: Vec<(usize, String, Option<u32>)>,
    pub paged: bool,
    /// PDF pages covered by images with no selectable text, excluding empty pages.
    pub scanned_pages: Vec<u32>,
}

impl OpenedDocument {
    fn info<'a>(&'a self, source: &'a str) -> DocumentInfo<'a> {
        DocumentInfo {
            source: if source.is_empty() { "input" } else { source },
            format: self.format,
            title: self.title.as_deref(),
            unit_noun: self.unit_noun,
            total: self.units.len() as u32,
            paged: self.paged,
        }
    }

    pub fn outline(&self, source: &str) -> Outline {
        Outline::from_units(self.info(source), &self.units, &self.native_outline)
    }

    /// Render the complete document with front matter and a notice for sparse PDFs.
    /// Use [Self::scanned_pages] to distinguish actual image scans from sparse text.
    pub fn render_markdown(&self, source: &str) -> String {
        let info = self.info(source);
        let mut header = info.header(&self.metadata);
        let markers = info.has_markers();
        if markers {
            let key = format!("{}s", self.unit_noun);
            header.retain(|(existing, _)| *existing != key);
            header.push((key, info.total.to_string()));
        }
        let mut out = front_matter(&header, &[]);
        for unit in &self.units {
            if markers {
                out.push_str(&crate::outline::unit_marker(&unit.label));
            }
            out.push_str(unit.markdown.trim());
            out.push_str("\n\n");
        }
        let total = self.units.len();
        if self.paged && total > 0 {
            let visible = self
                .units
                .iter()
                .flat_map(|unit| unit.markdown.chars())
                .filter(|c| c.is_alphanumeric())
                .count();
            if visible < total * 20 {
                out.push_str(
                    "<!-- These pages have little or no selectable text (scanned or image-only). \
The anymd CLI can OCR them with a local `tesseract`. -->\n\n",
                );
            }
        }
        out.truncate(out.trim_end().len());
        out.push('\n');
        out
    }
}

/// Open bytes with no OCR, geometry extraction, or page rendering. Enable only
/// `formats` (not `native`) for a socket-free core. Build without
/// `anymd-formats/native` to keep media conversion in memory too.
pub fn open_bytes(bytes: &[u8], options: &OpenBytesOptions) -> Result<OpenedDocument, OpenError> {
    let path = options
        .filename
        .as_deref()
        .filter(|name| !name.is_empty())
        .map(Path::new);
    let head = &bytes[..bytes.len().min(8192)];
    let format = if head.starts_with(b"%PDF-") {
        Some(Format::Pdf)
    } else {
        options
            .content_type
            .as_deref()
            .and_then(format_from_content_type)
            // Generic text, e.g. a default from an attachment service, must not
            // override a more specific signature or filename extension.
            .filter(|format| *format != Format::Text)
            .or_else(|| anymd_formats::detect(path, head))
    }
    .ok_or(OpenError::UnrecognizedFormat)?;
    let mut opened = if format == Format::Pdf {
        open_pdf(bytes)?
    } else {
        let converted = anymd_formats::convert(
            format,
            bytes,
            &Options {
                revisions: options.revisions,
                ..Options::default()
            },
        )
        .map_err(|source: ConvertError| OpenError::Conversion {
            format: format.name(),
            source: Box::new(source),
        })?;
        let units = units_from_sections(converted.sections);
        OpenedDocument {
            format: format.name(),
            title: converted.title,
            unit_noun: noun_for_units(&units, false),
            metadata: converted.metadata,
            units,
            native_outline: converted.outline,
            paged: false,
            scanned_pages: Vec::new(),
        }
    };
    if opened.title.is_none() {
        opened.title = title_from_units(&opened.units);
    }
    Ok(opened)
}

fn open_pdf(bytes: &[u8]) -> Result<OpenedDocument, OpenError> {
    let doc = anymd_pdf::load_document_bytes(bytes).map_err(OpenError::PdfParse)?;
    let numbers: Vec<u32> = (1..=anymd_pdf::page_count(&doc)).collect();
    let mut units = Vec::with_capacity(numbers.len());
    for chunk in numbers.chunks(PAGE_CHUNK) {
        let converted = anymd_pdf::pdf_to_markdown(&doc, Some(chunk)).map_err(|source| {
            OpenError::Conversion {
                format: "pdf",
                source: Box::new(source),
            }
        })?;
        units.extend(converted.pages.into_iter().map(Unit::from));
    }
    Ok(OpenedDocument {
        format: "pdf",
        title: anymd_pdf::info_title(&doc),
        unit_noun: noun_for_units(&units, true),
        metadata: Vec::new(),
        scanned_pages: scanned_pdf_pages(&doc, &units),
        units,
        native_outline: anymd_pdf::outline(&doc),
        paged: true,
    })
}

impl From<MarkdownPage> for Unit {
    fn from(page: MarkdownPage) -> Self {
        Self {
            number: page.number,
            label: format!("page {}", page.number),
            markdown: page.markdown,
        }
    }
}

pub fn units_from_sections(sections: Vec<Section>) -> Vec<Unit> {
    sections
        .into_iter()
        .enumerate()
        .map(|(index, section)| Unit {
            number: index as u32 + 1,
            label: section.label,
            markdown: section.markdown,
        })
        .collect()
}

/// Inspect only the units where neither extraction nor OCR supplied text.
pub fn scanned_pdf_pages(doc: &PdfDocument, units: &[Unit]) -> Vec<u32> {
    let blank: Vec<_> = units
        .iter()
        .filter(|unit| !unit.markdown.chars().any(char::is_alphanumeric))
        .map(|unit| unit.number)
        .collect();
    if blank.is_empty() {
        return Vec::new();
    }
    anymd_pdf::scanned_pages(doc, &blank)
}

/// Map a specific MIME type to a converter, ignoring optional MIME parameters.
pub fn format_from_content_type(content_type: &str) -> Option<Format> {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Some(match mime.as_str() {
        "application/pdf" | "application/x-pdf" => Format::Pdf,
        "text/html" | "application/xhtml+xml" => Format::Html,
        "text/csv" => Format::Csv,
        "text/tab-separated-values" => Format::Tsv,
        "application/epub+zip" => Format::Epub,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Format::Docx,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Format::Pptx,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.ms-excel"
        | "application/vnd.oasis.opendocument.spreadsheet" => Format::Xlsx,
        "text/vtt" | "application/x-subrip" => Format::Subtitles,
        "text/plain" | "text/markdown" | "application/json" | "text/xml" | "application/xml" => {
            Format::Text
        }
        m if m.starts_with("image/") => Format::Image,
        m if m.starts_with("video/") || m.starts_with("audio/") => Format::Video,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
