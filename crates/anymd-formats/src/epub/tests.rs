use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

use super::package::{join_path, percent_decode};
use super::{convert, convert_with_limits};
use crate::{ConvertError, Options, Section};

pub(crate) fn build_epub(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        for (name, body) in files {
            zip.start_file(*name, deflated).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    out.into_inner()
}

const CONTAINER: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

fn repeated_book(chapter_bytes: usize, repeats: usize) -> Vec<u8> {
    let opf = format!(
        "<package><manifest><item id='c' href='c.xhtml' media-type='application/xhtml+xml'/></manifest><spine>{}</spine></package>",
        "<itemref idref='c'/>".repeat(repeats)
    );
    let chapter = format!("<p>{}</p>", "a".repeat(chapter_bytes));
    build_epub(&[
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", &opf),
        ("OEBPS/c.xhtml", &chapter),
    ])
}

#[test]
fn cumulative_output_limit() {
    let epub = repeated_book(64 * 1024, 257);
    let error = match convert_with_limits(
        &epub,
        &Options::default(),
        crate::limits::Limits::with_output_limit(16 * 1024 * 1024),
    ) {
        Err(error) => error,
        Ok(converted) => panic!(
            "conversion exceeded the ceiling: {} input bytes produced {} Markdown bytes",
            epub.len(),
            converted
                .sections
                .iter()
                .map(|s| s.markdown.len())
                .sum::<usize>()
        ),
    };
    assert_eq!(
        error.to_string(),
        "conversion output exceeds 16777216 bytes"
    );
}

#[test]
#[ignore = "measured separately in the conversion limits CI job"]
fn production_cumulative_output_limit() {
    let epub = repeated_book(64 * 1024, 4097);
    let started = std::time::Instant::now();
    let error = crate::convert(crate::Format::Epub, &epub, &Options::default()).unwrap_err();
    assert_eq!(
        error,
        ConvertError::limit("output", crate::limits::MAX_OUTPUT_BYTES)
    );
    println!(
        "PRODUCTION_BOUND input={} elapsed={:?} error={error}",
        epub.len(),
        started.elapsed()
    );
}

#[test]
fn large_distinct_chapters_keep_their_bytes() {
    let text = "a".repeat(1024 * 1024);
    let mut manifest = String::new();
    let mut spine = String::new();
    let mut chapters = Vec::new();
    for n in 0..32 {
        manifest.push_str(&format!(
            "<item id='c{n}' href='c{n}.xhtml' media-type='application/xhtml+xml'/>"
        ));
        spine.push_str(&format!("<itemref idref='c{n}'/>"));
        chapters.push((format!("OEBPS/c{n}.xhtml"), format!("<p>{n} {text}</p>")));
    }
    let opf = format!("<package><manifest>{manifest}</manifest><spine>{spine}</spine></package>");
    let mut files = vec![
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", &opf),
    ];
    files.extend(
        chapters
            .iter()
            .map(|(name, body)| (name.as_str(), body.as_str())),
    );
    let converted = crate::convert(
        crate::Format::Epub,
        &build_epub(&files),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(converted.sections.len(), 32);
    for (n, section) in converted.sections.iter().enumerate() {
        assert_eq!(section.markdown, format!("{n} {text}"));
    }
}

#[test]
fn repeated_chapter_control() {
    let converted = convert(&repeated_book(5, 2), &Options::default()).unwrap();
    assert_eq!(
        converted.sections,
        vec![
            Section {
                label: "chapter 1".into(),
                markdown: "aaaaa".into()
            },
            Section {
                label: "chapter 2".into(),
                markdown: "aaaaa".into()
            },
        ]
    );
}

const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Tales &amp; Roads</dc:title>
    <dc:creator>Ann Author</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="cover" href="text/cover.xhtml" media-type="application/xhtml+xml"/>
    <item id="c1" href="text/chapter%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/c2.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine><itemref idref="nav"/><itemref idref="cover"/><itemref idref="c2"/><itemref idref="c1"/><itemref idref="missing"/></spine>
</package>"#;

#[test]
fn follows_spine_order_and_labels_chapters() {
    let epub = build_epub(&[
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", OPF),
        (
            "OEBPS/nav.xhtml",
            "<html><body><nav><ol><li><a href='text/c2.xhtml'>Two</a></li></ol></nav></body></html>",
        ),
        (
            "OEBPS/text/cover.xhtml",
            "<html><body><img src='../img/cover.jpg' alt='Cover'/></body></html>",
        ),
        (
            "OEBPS/text/chapter 1.xhtml",
            "<html><head><title>Book</title></head><body><h1>The Road</h1><p>It was <em>long</em>. See <a href='c2.xhtml#n1'>note</a>.</p></body></html>",
        ),
        (
            "OEBPS/text/c2.xhtml",
            "<html><body><h2>Before</h2><p>First &amp; foremost.</p></body></html>",
        ),
    ]);
    let converted = convert(&epub, &Options::default()).unwrap();
    assert_eq!(converted.format, "epub");
    assert_eq!(converted.outline, vec![(0, "Two".into(), Some(1))]);
    assert_eq!(converted.title.as_deref(), Some("Tales & Roads"));
    assert_eq!(
        converted.metadata[0],
        ("author".into(), "Ann Author".into())
    );
    let labels: Vec<&str> = converted
        .sections
        .iter()
        .map(|s| s.label.as_str())
        .collect();
    assert_eq!(labels, ["chapter 1: Before", "chapter 2: The Road"]);
    assert_eq!(
        converted.sections[0].markdown,
        "## Before\n\nFirst & foremost."
    );
    assert_eq!(
        converted.sections[1].markdown,
        "# The Road\n\nIt was *long*. See note."
    );
}

#[test]
fn native_tocs_preserve_nesting_and_prefer_toc_over_landmarks() {
    let nav = "<html><body><nav epub:type='landmarks'><ol><li><a href='text/c2.xhtml'>Skip</a></li></ol></nav><nav epub:type='toc'><ol><li><span>Part</span><ol><li><a href='text/c2.xhtml'>Before &amp; after</a><ol><li><a href='text/chapter%201.xhtml#road'>Road</a></li></ol></li></ol></li></ol></nav></body></html>";
    let epub = build_epub(&[
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", OPF),
        ("OEBPS/nav.xhtml", nav),
        ("OEBPS/text/c2.xhtml", "<h1>Before</h1><p>First.</p>"),
        (
            "OEBPS/text/chapter 1.xhtml",
            "<h1>The Road</h1><p>Second.</p>",
        ),
    ]);
    let converted = convert(&epub, &Options::default()).unwrap();
    assert_eq!(
        converted.outline,
        vec![
            (0, "Part".into(), Some(1)),
            (1, "Before & after".into(), Some(1)),
            (2, "Road".into(), Some(2))
        ]
    );
}

#[test]
fn reads_epub2_ncx_without_changing_chapter_markdown() {
    let opf = OPF.replace(
        "href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"",
        "href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"",
    );
    let ncx = "<ncx><navMap><navPoint><navLabel><text>Before &amp; after</text></navLabel><content src='text/c2.xhtml'/><navPoint><navLabel><text><![CDATA[Road]]></text></navLabel><content src='text/chapter%201.xhtml#road'/></navPoint></navPoint></navMap></ncx>";
    let epub = build_epub(&[
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", &opf),
        ("OEBPS/toc.ncx", ncx),
        ("OEBPS/text/c2.xhtml", "<h1>Before</h1><p>First.</p>"),
        (
            "OEBPS/text/chapter 1.xhtml",
            "<h1>The Road</h1><p>Second.</p>",
        ),
    ]);
    let converted = convert(&epub, &Options::default()).unwrap();
    assert_eq!(
        converted.outline,
        vec![
            (0, "Before & after".into(), Some(1)),
            (1, "Road".into(), Some(2))
        ]
    );
    assert_eq!(converted.sections[0].markdown, "# Before\n\nFirst.");
}

#[test]
fn rejects_garbage_and_bounds() {
    assert!(matches!(
        convert(b"not a zip", &Options::default()),
        Err(ConvertError::Invalid(_))
    ));
    let no_opf = build_epub(&[("hello.txt", "hi")]);
    assert!(matches!(
        convert(&no_opf, &Options::default()),
        Err(ConvertError::Invalid(_))
    ));
    let bad_opf = build_epub(&[
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", "<package><spine>"),
    ]);
    assert!(convert(&bad_opf, &Options::default()).is_err());
}

#[test]
fn path_helpers() {
    assert_eq!(join_path("OEBPS", "../img/a.png"), "img/a.png");
    assert_eq!(join_path("", "text/./a.xhtml"), "text/a.xhtml");
    assert_eq!(percent_decode("chapter%201%2"), "chapter 1%2");
    assert_eq!(percent_decode("%é%"), "%é%");
    assert_eq!(percent_decode("%E4%B8%AD"), "中");
}
