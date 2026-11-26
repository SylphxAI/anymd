use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

use super::package::{join_path, percent_decode};
use super::{convert, convert_with_limits};
use crate::{ConvertError, Options, Section};

pub(crate) fn build_epub(files: &[(&str, &str)]) -> Vec<u8> {
    zip_epub(files.iter().map(|&(name, body)| (name, body.as_bytes())))
}

fn zip_epub<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Vec<u8> {
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
            zip.start_file(name, deflated).unwrap();
            zip.write_all(body).unwrap();
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
  <spine><itemref idref="nav" linear="no"/><itemref idref="cover"/><itemref idref="c2"/><itemref idref="c1"/><itemref idref="missing"/></spine>
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

const VISIBLE_NAV: &str = r#"<html><body>
<nav epub:type="toc" role="navigation" class="nav">
  <h2>Paths through this book</h2>
  <ol>
    <li><a href="text/c2.xhtml#arrival">Read the arrival first</a></li>
    <li><a href="text/chapter%201.xhtml#road">Return to the road</a></li>
  </ol>
</nav>
<nav epub:type="landmarks" hidden="hidden"><p>Hidden landmark</p></nav>
<nav epub:type="page-list" style="display: none"><p>Hidden page list</p></nav>
<p>A note from the editor. See <a href="https://example.org/guide">the guide</a>.</p>
</body></html>"#;

const ROAD: &str =
    "<h1 id='road'>The Road</h1><p>A winding road.</p><img src='../image.png' alt='Route map'>";
const ARRIVAL: &str =
    "<h1 id='arrival'>The Arrival</h1><p>Safe at last.</p><img src='../image.png' alt='Route map'>";

fn nav_book(nav_entry: &str, nav: &str, image: &[u8]) -> Vec<u8> {
    let opf = OPF.replace(
        "<itemref idref=\"nav\" linear=\"no\"/><itemref idref=\"cover\"/><itemref idref=\"c2\"/><itemref idref=\"c1\"/><itemref idref=\"missing\"/>",
        &format!("<itemref idref='c1'/>{nav_entry}<itemref idref='c2'/>"),
    );
    zip_epub([
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/nav.xhtml", nav.as_bytes()),
        ("OEBPS/text/chapter 1.xhtml", ROAD.as_bytes()),
        ("OEBPS/text/c2.xhtml", ARRIVAL.as_bytes()),
        ("OEBPS/image.png", image),
    ])
}

#[test]
fn renders_visible_linear_navigation_in_spine_order() {
    for entry in [
        "<itemref idref='nav'/>",
        "<itemref idref='nav' linear='yes'/>",
    ] {
        let epub = nav_book(entry, VISIBLE_NAV, &[]);
        let converted = convert(&epub, &Options::default()).unwrap();
        let body: Vec<_> = converted
            .sections
            .iter()
            .map(|s| s.markdown.as_str())
            .collect();
        assert_eq!(body, [
            "# The Road\n\nA winding road.\n\n![Route map](../image.png)",
            "## Paths through this book\n\n1. Read the arrival first\n2. Return to the road\n\nA note from the editor. See [the guide](https://example.org/guide).",
            "# The Arrival\n\nSafe at last.\n\n![Route map](../image.png)",
        ]);
        assert_eq!(
            converted.outline,
            vec![
                (0, "Read the arrival first".into(), Some(3)),
                (0, "Return to the road".into(), Some(1)),
            ]
        );
    }
}

#[test]
fn outline_only_navigation_does_not_create_a_body_section() {
    let hidden = "<nav epub:type='toc' hidden><h2>Hidden contents</h2><ol><li><a href='text/c2.xhtml#arrival'>The Arrival</a></li></ol></nav>";
    for (entry, nav, label) in [
        ("", VISIBLE_NAV, "Read the arrival first"),
        (
            "<itemref idref='nav' linear='no'/>",
            VISIBLE_NAV,
            "Read the arrival first",
        ),
        ("<itemref idref='nav'/>", hidden, "The Arrival"),
    ] {
        let converted = convert(&nav_book(entry, nav, &[]), &Options::default()).unwrap();
        let labels: Vec<_> = converted
            .sections
            .iter()
            .map(|s| s.label.as_str())
            .collect();
        assert_eq!(labels, ["chapter 1: The Road", "chapter 2: The Arrival"]);
        assert_eq!(converted.outline[0], (0, label.into(), Some(2)));
    }
}

#[cfg(feature = "native")]
#[test]
fn exports_chapter_image_with_linear_navigation_in_the_spine() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = crate::images::tests::png(64, 64);
    let nav = VISIBLE_NAV.replace("</body>", "<img src='image.png' alt='Route map'></body>");
    let epub = nav_book("<itemref idref='nav'/>", &nav, &bytes);
    let converted = convert(
        &epub,
        &Options {
            images: Some(crate::images::ImageStore::at(dir.path())),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(converted.sections.len(), 3);
    let image = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(std::fs::read(&image).unwrap(), bytes);
    for (i, section) in converted.sections.iter().enumerate() {
        let chapter = i + 1;
        assert!(
            section.markdown.ends_with(&format!(
                "![Route map]({})\n<!-- image: 64x64, chapter {chapter} -->",
                image.display()
            )),
            "{}",
            section.markdown
        );
    }
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
