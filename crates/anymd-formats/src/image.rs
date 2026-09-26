//! Images → a short metadata table (format, size, EXIF essentials) plus opt-in OCR.
//!
//! Ported in spirit from iris `image-reader-core`: header-only dimensions and
//! kamadak-exif fields, without decoding pixels.

use std::io::Cursor;
use std::time::Duration;

use exif::{In, Tag, Value};

use crate::{tool, ConvertError, Converted, Options, Section};

const OCR_TIMEOUT: Duration = Duration::from_secs(60);

/// True when the leading bytes identify this media kind.
pub fn sniff(head: &[u8]) -> bool {
    kind(head).is_some()
}

/// Short image kind from magic bytes.
fn kind(head: &[u8]) -> Option<&'static str> {
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
    facts.extend(exif_facts(bytes));

    let mut rows = vec![vec!["Property".to_string(), "Value".to_string()]];
    rows.extend(facts.iter().map(|(k, v)| vec![capitalize(k), v.clone()]));
    let mut markdown = crate::markdown_table(&rows).trim_end().to_string();
    markdown.push_str("\n\n");
    if options.ocr {
        markdown.push_str(&ocr_section(bytes, &format));
    } else if cfg!(feature = "native") {
        markdown.push_str("_Image text is not extracted by default; pass `ocr: true` to OCR it (needs a local `tesseract`)._");
    } else {
        markdown.push_str("_Image text is not extracted in the browser; the anymd CLI can OCR it with a local `tesseract` (`anymd --ocr`)._");
    }

    Ok(Converted {
        format: "image".into(),
        title: None,
        sections: vec![Section {
            label: "image".into(),
            markdown,
        }],
        metadata: facts,
    })
}

fn ocr_section(bytes: &[u8], format: &str) -> String {
    let Some(tesseract) = tool::find("tesseract") else {
        return "_OCR was requested but needs `tesseract` installed (e.g. `apt install tesseract-ocr` or `brew install tesseract`)._".into();
    };
    let suffix = format!(".{}", format.to_ascii_lowercase());
    let result = tool::temp_file(bytes, &suffix).and_then(|file| {
        let path = file.path().as_os_str().to_owned();
        let output = tool::run(
            &tesseract,
            [path.as_os_str(), "stdout".as_ref()],
            OCR_TIMEOUT,
        )?;
        if output.success {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(format!(
                "tesseract failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    });
    match result {
        Ok(text) => {
            let text = tidy_ocr(&text);
            if text.is_empty() {
                "## Text (OCR)\n\n_No text found._".into()
            } else {
                format!("## Text (OCR)\n\n{text}")
            }
        }
        Err(error) => format!("_OCR failed: {error}_"),
    }
}

/// True when a local `tesseract` binary is on PATH.
pub fn ocr_available() -> bool {
    tool::find("tesseract").is_some()
}

/// OCR an image (PNG/JPEG/...) with the local `tesseract`; returns tidy text.
pub fn ocr_text(bytes: &[u8], suffix: &str) -> Result<String, String> {
    let tesseract = tool::find("tesseract")
        .ok_or_else(|| "OCR needs `tesseract` installed (e.g. `apt install tesseract-ocr` or `brew install tesseract`).".to_string())?;
    let file = tool::temp_file(bytes, suffix)?;
    let path = file.path().as_os_str().to_owned();
    let output = tool::run(&tesseract, [path.as_os_str(), "stdout".as_ref()], OCR_TIMEOUT)?;
    if output.success {
        Ok(tidy_ocr(&String::from_utf8_lossy(&output.stdout)))
    } else {
        Err(format!(
            "tesseract failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
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

/// OCR an image with the local `tesseract` and return every word with its
/// box, so the caller can lay the page out (paragraphs, columns, tables)
/// instead of taking tesseract's plain text. One tesseract thread: callers
/// run several pages at once.
pub fn ocr_words(bytes: &[u8], suffix: &str) -> Result<Vec<OcrWord>, String> {
    let tesseract = tool::find("tesseract")
        .ok_or_else(|| "OCR needs `tesseract` installed (e.g. `apt install tesseract-ocr` or `brew install tesseract`).".to_string())?;
    let file = tool::temp_file(bytes, suffix)?;
    let path = file.path().as_os_str().to_owned();
    let output = tool::run_with_env(
        &tesseract,
        [path.as_os_str(), "stdout".as_ref(), "tsv".as_ref()],
        &[("OMP_THREAD_LIMIT", "1")],
        OCR_TIMEOUT,
    )?;
    if !output.success {
        return Err(format!(
            "tesseract failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(parse_tsv(&String::from_utf8_lossy(&output.stdout)))
}

/// Words (level 5 rows) from tesseract's TSV output.
pub fn parse_tsv(tsv: &str) -> Vec<OcrWord> {
    let mut out = Vec::new();
    for line in tsv.lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 12 || fields[0] != "5" {
            continue;
        }
        let number = |i: usize| fields[i].trim().parse::<u32>().ok();
        let (Some(block), Some(par), Some(row), Some(left), Some(top), Some(width), Some(height)) = (
            number(2),
            number(3),
            number(4),
            number(6),
            number(7),
            number(8),
            number(9),
        ) else {
            continue;
        };
        let confidence = fields[10].trim().parse::<f32>().unwrap_or(-1.0);
        let text = fields[11..].join("\t").trim().to_string();
        if text.is_empty() || confidence < 0.0 {
            continue;
        }
        out.push(OcrWord {
            left,
            top,
            width,
            height,
            line: (block, par, row),
            confidence,
            text,
        });
    }
    out
}

fn tidy_ocr(text: &str) -> String {
    let mut out = Vec::new();
    let mut blank = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push(String::new());
            blank = false;
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

fn exif_facts(bytes: &[u8]) -> Vec<(String, String)> {
    let Ok(exif) = exif::Reader::new().read_from_container(&mut Cursor::new(bytes)) else {
        return Vec::new();
    };
    let text = |tag: Tag| -> Option<String> {
        let field = exif.get_field(tag, In::PRIMARY)?;
        let value = match &field.value {
            Value::Ascii(chunks) => chunks
                .iter()
                .map(|c| {
                    String::from_utf8_lossy(c)
                        .trim_end_matches('\0')
                        .trim()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(" "),
            _ => field.display_value().with_unit(&exif).to_string(),
        };
        let value = value.trim().trim_matches('"').trim().to_string();
        (!value.is_empty()).then_some(value)
    };
    let mut facts = Vec::new();

    let make = text(Tag::Make);
    let model = text(Tag::Model);
    let camera = match (make, model) {
        (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => {
            Some(model)
        }
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (make, model) => make.or(model),
    };
    if let Some(camera) = camera {
        facts.push(("camera".into(), camera));
    }
    if let Some(lens) = text(Tag::LensModel) {
        facts.push(("lens".into(), lens));
    }
    if let Some(taken) = text(Tag::DateTimeOriginal).or_else(|| text(Tag::DateTime)) {
        facts.push(("taken".into(), taken));
    }
    let exposure: Vec<String> = [
        text(Tag::ExposureTime),
        text(Tag::FNumber),
        text(Tag::PhotographicSensitivity).map(|iso| format!("ISO {iso}")),
        text(Tag::FocalLength),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !exposure.is_empty() {
        facts.push(("exposure".into(), exposure.join(", ")));
    }
    if let Some(orientation) = exif
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .and_then(orientation_text)
    {
        facts.push(("orientation".into(), orientation.into()));
    }
    if let Some(gps) = gps(&exif) {
        facts.push(("gps".into(), gps));
    }
    for (key, tag) in [
        ("description", Tag::ImageDescription),
        ("artist", Tag::Artist),
        ("copyright", Tag::Copyright),
        ("software", Tag::Software),
    ] {
        if let Some(value) = text(tag) {
            facts.push((key.into(), value.chars().take(200).collect()));
        }
    }
    facts
}

fn orientation_text(value: u32) -> Option<&'static str> {
    Some(match value {
        2 => "mirrored horizontally",
        3 => "rotated 180°",
        4 => "mirrored vertically",
        5 => "mirrored, rotated 90° CW",
        6 => "rotated 90° CW",
        7 => "mirrored, rotated 90° CCW",
        8 => "rotated 90° CCW",
        _ => return None,
    })
}

fn gps(exif: &exif::Exif) -> Option<String> {
    let coord = |tag: Tag, reference: Tag, negative: u8| -> Option<f64> {
        let field = exif.get_field(tag, In::PRIMARY)?;
        let Value::Rational(parts) = &field.value else {
            return None;
        };
        let part = |i: usize| {
            parts
                .get(i)
                .filter(|r| r.denom != 0)
                .map(|r| r.to_f64())
                .unwrap_or(0.0)
        };
        let mut degrees = part(0) + part(1) / 60.0 + part(2) / 3600.0;
        let sign = exif
            .get_field(reference, In::PRIMARY)
            .and_then(|f| match &f.value {
                Value::Ascii(chunks) => chunks.first().and_then(|c| c.first().copied()),
                _ => None,
            });
        if sign == Some(negative) {
            degrees = -degrees;
        }
        degrees.is_finite().then_some(degrees)
    };
    let lat = coord(Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S')?;
    let lon = coord(Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W')?;
    let mut out = format!("{lat:.6}, {lon:.6}");
    if let Some(alt) = exif
        .get_field(Tag::GPSAltitude, In::PRIMARY)
        .and_then(|f| match &f.value {
            Value::Rational(r) => r.first().filter(|r| r.denom != 0).map(|r| r.to_f64()),
            _ => None,
        })
    {
        out.push_str(&format!(" (altitude {alt:.0} m)"));
    }
    Some(out)
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
mod tests {
    use super::*;
    use exif::experimental::Writer;
    use exif::{Field, Rational};

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = ::image::RgbImage::new(width, height);
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ::image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// A JPEG shell (SOI, APP1 Exif, SOF0, EOI): enough for header readers.
    fn jpeg_with_exif() -> Vec<u8> {
        let fields = [
            Field {
                tag: Tag::Make,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![b"Canon".to_vec()]),
            },
            Field {
                tag: Tag::Model,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![b"Canon EOS R5".to_vec()]),
            },
            Field {
                tag: Tag::Orientation,
                ifd_num: In::PRIMARY,
                value: Value::Short(vec![6]),
            },
            Field {
                tag: Tag::DateTimeOriginal,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![b"2024:05:01 10:20:30".to_vec()]),
            },
            Field {
                tag: Tag::GPSLatitudeRef,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![b"N".to_vec()]),
            },
            Field {
                tag: Tag::GPSLatitude,
                ifd_num: In::PRIMARY,
                value: Value::Rational(vec![
                    Rational { num: 22, denom: 1 },
                    Rational { num: 18, denom: 1 },
                    Rational { num: 0, denom: 1 },
                ]),
            },
            Field {
                tag: Tag::GPSLongitudeRef,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![b"E".to_vec()]),
            },
            Field {
                tag: Tag::GPSLongitude,
                ifd_num: In::PRIMARY,
                value: Value::Rational(vec![
                    Rational { num: 114, denom: 1 },
                    Rational { num: 10, denom: 1 },
                    Rational { num: 0, denom: 1 },
                ]),
            },
        ];
        let mut writer = Writer::new();
        for field in &fields {
            writer.push_field(field);
        }
        let mut tiff = Cursor::new(Vec::new());
        writer.write(&mut tiff, false).unwrap();
        let tiff = tiff.into_inner();

        let mut jpeg = vec![0xFF, 0xD8];
        let app1_len = (2 + 6 + tiff.len()) as u16;
        jpeg.extend([0xFF, 0xE1]);
        jpeg.extend(app1_len.to_be_bytes());
        jpeg.extend(b"Exif\0\0");
        jpeg.extend(&tiff);
        // SOF0: 8-bit, 300 high, 400 wide, 1 component.
        jpeg.extend([
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x01, 0x2C, 0x01, 0x90, 0x01, 0x01, 0x11, 0x00,
        ]);
        jpeg.extend([0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn sniffs_magic_bytes() {
        assert!(sniff(&png(1, 1)));
        assert!(sniff(b"GIF89a...."));
        assert!(sniff(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(sniff(b"\0\0\0\x1cftypavif\0\0\0\0"));
        assert!(!sniff(b"\0\0\0\x1cftypisom\0\0\0\0"));
        assert!(!sniff(b"BMW is a car maker, not a bitmap."));
        let mut bmp = b"BM".to_vec();
        bmp.extend([0x46, 0, 0, 0, 0, 0, 0, 0, 0x36, 0, 0, 0, 40, 0, 0, 0]);
        assert!(sniff(&bmp));
    }

    #[test]
    fn png_reports_dimensions_and_ocr_hint() {
        let converted = convert(&png(64, 32), &Options::default()).unwrap();
        assert_eq!(converted.format, "image");
        let md = &converted.sections[0].markdown;
        assert!(md.starts_with("|Property|Value|\n|-|-|\n|Format|PNG|\n|Dimensions|64 × 32 px|"), "{md}");
        assert!(md.contains("`ocr: true`"));
    }

    #[test]
    fn jpeg_exif_essentials() {
        let converted = convert(&jpeg_with_exif(), &Options::default()).unwrap();
        let get = |k: &str| {
            converted
                .metadata
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("format"), Some("JPEG"));
        assert_eq!(get("dimensions"), Some("400 × 300 px"));
        assert_eq!(get("camera"), Some("Canon EOS R5"));
        assert_eq!(get("taken"), Some("2024:05:01 10:20:30"));
        assert_eq!(get("orientation"), Some("rotated 90° CW"));
        assert_eq!(get("gps"), Some("22.300000, 114.166667"));
    }

    #[test]
    fn ocr_without_tesseract_or_with_it_never_errors() {
        let converted = convert(
            &png(8, 8),
            &Options {
                ocr: true,
                ..Options::default()
            },
        )
        .unwrap();
        let md = &converted.sections[0].markdown;
        assert!(
            md.contains("tesseract") || md.contains("## Text (OCR)") || md.contains("OCR failed"),
            "{md}"
        );
    }

    #[test]
    fn garbage_is_invalid() {
        assert!(matches!(
            convert(b"hello", &Options::default()),
            Err(ConvertError::Invalid(_))
        ));
        assert!(convert(b"\x89PNG\r\n\x1a\n", &Options::default()).is_ok());
    }
}
