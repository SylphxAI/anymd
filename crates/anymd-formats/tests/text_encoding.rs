//! Synthetic text documents must mean the same thing in every BOM encoding.
use std::path::Path;

use anymd_formats::{convert, detect, html, Format, Options};

const TEXT: &str = "# Records\r\n\r\nAlice in Hong Kong; 陳小明在香港; amber-cobalt-729. 😀𠮷\r\n";
const HTML: &str = "<!doctype html>\r\n<html><head><title>Records 陳小明😀</title></head><body><main><h1>Records</h1><p>Alice in Hong Kong; 陳小明在香港; amber-cobalt-729. 😀𠮷</p><table><tr><th>Name</th><th>City</th></tr><tr><td>陳小明😀</td><td>香港</td></tr></table><script>unsafe-marker</script><a href=\"javascript:alert(1)\">safe label</a></main></body></html>\r\n";

fn encodings(text: &str) -> Vec<Vec<u8>> {
    let mut le = vec![0xff, 0xfe];
    let mut be = vec![0xfe, 0xff];
    for unit in text.encode_utf16() {
        le.extend(unit.to_le_bytes());
        be.extend(unit.to_be_bytes());
    }
    vec![
        text.as_bytes().to_vec(),
        format!("\u{feff}{text}").into_bytes(),
        le,
        be,
    ]
}

#[test]
fn same_text_and_html_structure_in_every_encoding() {
    for (extension, text, format) in [
        ("txt", TEXT, Format::Text),
        ("md", TEXT, Format::Text),
        ("html", HTML, Format::Html),
        ("htm", HTML, Format::Html),
    ] {
        let expected = convert(format, text.as_bytes(), &Options::default()).unwrap();
        for bytes in encodings(text) {
            assert_eq!(
                detect(Some(Path::new(&format!("records.{extension}"))), &bytes),
                Some(format)
            );
            assert_eq!(
                convert(format, &bytes, &Options::default()).unwrap(),
                expected
            );
            if format == Format::Html {
                assert_eq!(
                    html::convert(&bytes, &Options::default()).unwrap(),
                    expected
                );
            } else {
                assert_eq!(expected.sections[0].markdown, TEXT); // CRLF and supplementary characters unchanged.
            }
        }
        if format == Format::Html {
            assert_eq!(expected.title.as_deref(), Some("Records 陳小明😀"));
            let md = &expected.sections[0].markdown;
            assert!(md.contains("# Records"));
            assert!(md.contains("|陳小明😀|香港|"));
            assert!(!md.contains("<h1>"));
            assert!(!md.contains("unsafe-marker"));
            assert!(!md.contains("javascript:"));
        }
    }
}

#[test]
fn bom_precedes_ambiguous_audio_sync_but_not_binary_formats() {
    // 'A' and '#' after FF FE both satisfy the old loose MPEG frame probe.
    for text in ["Alice", "# Records", "\r\nRecords", "", HTML] {
        for bytes in encodings(text) {
            let expected = if text == HTML {
                Format::Html
            } else {
                Format::Text
            };
            assert_eq!(detect(None, &bytes), Some(expected));
            assert_eq!(
                detect(Some(Path::new("input.unknown")), &bytes),
                Some(expected)
            );
        }
    }
    for extension in ["csv", "tsv", "tab"] {
        let expected = if extension == "csv" {
            Format::Csv
        } else {
            Format::Tsv
        };
        for bytes in encodings("A,B\r\n1,2\r\n") {
            assert_eq!(
                detect(Some(Path::new(&format!("table.{extension}"))), &bytes),
                Some(expected)
            );
        }
    }
    for (bytes, format) in [
        (&b"ID3\x04\0\0\0\0\0\0"[..], Format::Video),
        (&b"\xff\xfb\x90\x64"[..], Format::Video),
        (&b"\xff\xfe\x90\x64"[..], Format::Video), // Valid MPEG-1 Layer I prefix, not a text file.
        (&b"\xff\xf1\x50\x80"[..], Format::Video),
        (&b"\x89PNG\r\n\x1a\n"[..], Format::Image),
        (&b"%PDF-1.7"[..], Format::Pdf),
    ] {
        let path = if bytes.starts_with(b"\xff\xfe") {
            "audio.mp3"
        } else {
            "misleading.txt"
        };
        assert_eq!(detect(Some(Path::new(path)), bytes), Some(format));
    }
    // A real frame sequence still wins over a misleading text extension.
    let mut frames = vec![0; 628];
    frames[..4].copy_from_slice(&[0xff, 0xfe, 0x90, 0x64]);
    frames[312..316].copy_from_slice(&[0xff, 0xfe, 0x90, 0x64]);
    assert_eq!(
        detect(Some(Path::new("audio.txt")), &frames),
        Some(Format::Video)
    );
    assert_eq!(detect(None, &frames), Some(Format::Video));
    assert_eq!(
        detect(Some(Path::new("binary.txt")), b"\x80\x81\0\x01"),
        None
    );
    for bytes in encodings("\0\u{1}\u{2}").into_iter().skip(2) {
        assert!(convert(Format::Text, &bytes, &Options::default()).is_err());
        assert!(html::convert(&bytes, &Options::default()).is_err());
    }
}

#[test]
fn strict_utf16_errors_and_recovery() {
    for format in [Format::Text, Format::Html] {
        for little in [true, false] {
            let bom = if little { [0xff, 0xfe] } else { [0xfe, 0xff] };
            for payload in [
                vec![0x41],
                if little { vec![0, 0xd8] } else { vec![0xd8, 0] },
                if little { vec![0, 0xdc] } else { vec![0xdc, 0] },
            ] {
                let bytes = [bom.to_vec(), payload].concat();
                let extension = if format == Format::Text {
                    "txt"
                } else {
                    "html"
                };
                assert_eq!(
                    detect(Some(Path::new(&format!("bad.{extension}"))), &bytes),
                    Some(format)
                );
                let error = convert(format, &bytes, &Options::default())
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("UTF-16"), "{error}");
                assert!(error.contains("re-export"), "{error}");
                assert!(
                    error.contains("odd byte count") || error.contains("unpaired surrogate"),
                    "{error}"
                );
                assert!(convert(format, TEXT.as_bytes(), &Options::default()).is_ok());
            }
        }
    }
}

#[test]
fn detection_head_can_end_mid_codepoint_without_corrupting_conversion() {
    for text in [
        format!("{}😀 end", "A".repeat(4094)),
        format!("{}陳 end", "A".repeat(8191)),
    ] {
        for bytes in encodings(&text) {
            let head = &bytes[..bytes.len().min(8192)];
            assert_eq!(detect(None, head), Some(Format::Text));
            assert_eq!(
                convert(Format::Text, &bytes, &Options::default())
                    .unwrap()
                    .sections[0]
                    .markdown,
                text
            );
        }
    }
}
