use std::io::Cursor;

use exif::experimental::Writer;
use exif::{Field, In, Rational, Tag, Value};

use super::{convert, parse_tsv, sniff, ConvertError, Options};

#[test]
fn tesseract_tsv_words_keep_boxes_and_lines() {
    let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
4\t1\t1\t1\t1\t0\t10\t20\t300\t40\t-1\t\n\
5\t1\t1\t1\t1\t1\t10\t20\t90\t40\t96.5\tNASA\n\
5\t1\t1\t1\t1\t2\t110\t20\t120\t40\t91\tProgram\n\
5\t1\t1\t1\t1\t3\t240\t20\t10\t40\t-1\t \n";
    let words = parse_tsv(tsv);
    assert_eq!(words.len(), 2);
    assert_eq!(words[1].text, "Program");
    assert_eq!(
        (words[1].left, words[1].width, words[1].line),
        (110, 120, (1, 1, 1))
    );
}

pub(super) fn png(width: u32, height: u32) -> Vec<u8> {
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
    assert!(
        md.starts_with("|Property|Value|\n|-|-|\n|Format|PNG|\n|Dimensions|64 × 32 px|"),
        "{md}"
    );
    if cfg!(feature = "native") {
        assert!(md.contains("`ocr: true`"), "{md}");
    } else {
        assert!(md.contains("`anymd --ocr`"), "{md}");
    }
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
