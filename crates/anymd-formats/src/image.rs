//! Images → a short metadata table (format, size, EXIF essentials) plus opt-in OCR.
//!
//! Ported in spirit from iris `image-reader-core`: header-only dimensions and
//! kamadak-exif fields, without decoding pixels.

use crate::{ConvertError, Converted, Options, Section};

mod decimal;
mod metadata;
mod ocr;

pub use decimal::restore_decimal_commas;
pub use ocr::{ocr_available, ocr_text, ocr_words, parse_tsv};

/// True when the leading bytes identify this media kind.
pub fn sniff(head: &[u8]) -> bool {
    kind(head).is_some()
}

/// Short image kind from magic bytes.
pub(crate) fn kind(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("PNG")
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("JPEG")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("GIF")
    } else if head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP" {
        Some("WebP")
    } else if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        Some("TIFF")
    } else if is_bmp(head) {
        Some("BMP")
    } else if head.len() >= 12 && &head[4..8] == b"ftyp" {
        match &head[8..12] {
            b"avif" | b"avis" => Some("AVIF"),
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => Some("HEIF"),
            _ => None,
        }
    } else {
        None
    }
}

/// "BM" alone is too weak (plain text can start with it); check the header shape.
fn is_bmp(head: &[u8]) -> bool {
    head.len() >= 18
        && head.starts_with(b"BM")
        && head[6..10] == [0, 0, 0, 0]
        && matches!(
            u32::from_le_bytes([head[14], head[15], head[16], head[17]]),
            12 | 40 | 52 | 56 | 64 | 108 | 124
        )
}

pub(crate) fn is_image_brand(brand: &[u8]) -> bool {
    matches!(
        brand,
        b"avif" | b"avis" | b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1"
    )
}

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    let dimensions = imagesize::blob_size(bytes).ok();
    let format = kind(bytes)
        .map(str::to_string)
        .or_else(|| imagesize::image_type(bytes).ok().map(|t| format!("{t:?}")))
        .ok_or_else(|| ConvertError::Invalid("not a recognized image".into()))?;

    let mut facts: Vec<(String, String)> = vec![("format".into(), format.clone())];
    if let Some(size) = dimensions {
        facts.push((
            "dimensions".into(),
            format!("{} × {} px", size.width, size.height),
        ));
    }
    facts.push(("file size".into(), human_bytes(bytes.len() as u64)));
    facts.extend(metadata::exif_facts(bytes));

    let mut rows = vec![vec!["Property".to_string(), "Value".to_string()]];
    rows.extend(facts.iter().map(|(k, v)| vec![capitalize(k), v.clone()]));
    let mut markdown = crate::markdown_table(&rows).trim_end().to_string();
    markdown.push_str("\n\n");
    if options.ocr {
        markdown.push_str(&ocr::ocr_section(bytes, &format));
    } else if cfg!(feature = "native") {
        markdown.push_str("_Image text is not extracted by default; pass `ocr: true` to OCR it (needs a local `tesseract`)._");
    } else {
        markdown.push_str("_Image text is not extracted in the browser; the anymd CLI can OCR it with a local `tesseract` (`anymd --ocr`)._");
    }

    Ok(Converted {
        outline: Vec::new(),
        format: "image".into(),
        title: None,
        sections: vec![Section {
            label: "image".into(),
            markdown,
        }],
        metadata: facts,
    })
}

/// One word found by OCR: its box in image pixels (y grows downward), the
/// text line it belongs to, and tesseract's confidence (0-100).
#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    /// Block, paragraph and line number: words that share it share a line.
    pub line: (u32, u32, u32),
    pub confidence: f32,
    pub text: String,
}

pub(crate) fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn capitalize(key: &str) -> String {
    let mut chars = key.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
