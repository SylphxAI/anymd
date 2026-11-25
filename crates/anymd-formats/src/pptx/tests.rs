use std::io::Write;

use crate::ooxml::{self, Element};
use crate::{ConvertError, Options};

use super::extras::chart_markdown;
use super::{convert, convert_with_limits};

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn zip(parts: &[(String, String)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, body) in parts {
        writer.start_file(name.as_str(), options).unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn repeated_deck(text_bytes: usize, repeats: usize) -> Vec<u8> {
    zip(&[
        (
            "ppt/presentation.xml".into(),
            format!(
                "<p:presentation {NS}><p:sldIdLst>{}</p:sldIdLst></p:presentation>",
                "<p:sldId r:id='rId1'/>".repeat(repeats)
            ),
        ),
        (
            "ppt/_rels/presentation.xml.rels".into(),
            rels(&[("rId1", "slide", "slides/slide1.xml")]),
        ),
        (
            "ppt/slides/slide1.xml".into(),
            format!(
                "<p:sld {NS}><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>",
                "a".repeat(text_bytes)
            ),
        ),
    ])
}

#[test]
fn cumulative_output_limit() {
    let bytes = repeated_deck(64 * 1024, 257);
    let error = match convert_with_limits(
        &bytes,
        &Options::default(),
        crate::limits::Limits::with_output_limit(16 * 1024 * 1024),
    ) {
        Err(error) => error,
        Ok(converted) => panic!(
            "conversion exceeded the ceiling: {} input bytes produced {} Markdown bytes",
            bytes.len(),
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
fn large_deck_keeps_repeated_slides() {
    let converted = crate::convert(
        crate::Format::Pptx,
        &repeated_deck(1024 * 1024, 32),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(converted.sections.len(), 32);
    let expected = format!("{}\n", "a".repeat(1024 * 1024));
    for section in converted.sections {
        assert_eq!(section.markdown, expected);
    }
}

fn rels(entries: &[(&str, &str, &str)]) -> String {
    let body: String = entries
        .iter()
        .map(|(id, kind, target)| {
            let mode = if target.starts_with("http") {
                r#" TargetMode="External""#
            } else {
                ""
            };
            format!(r#"<Relationship Id="{id}" Type="{REL}/{kind}" Target="{target}"{mode}/>"#)
        })
        .collect();
    format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{body}</Relationships>"#
    )
}

fn sp(ph: &str, y: i64, paragraphs: &str) -> String {
    let ph = if ph.is_empty() {
        String::new()
    } else {
        format!("<p:ph {ph}/>")
    };
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="s"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="{y}"/><a:ext cx="1" cy="1"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/>{paragraphs}</p:txBody></p:sp>"#
    )
}

fn para(lvl: u32, text: &str) -> String {
    format!(r#"<a:p><a:pPr lvl="{lvl}"/><a:r><a:rPr lang="en-US"/><a:t>{text}</a:t></a:r></a:p>"#)
}

fn slide(shapes: &str) -> String {
    format!(
        r#"<p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr/><p:grpSpPr/>{shapes}</p:spTree></p:cSld></p:sld>"#
    )
}

fn deck() -> Vec<u8> {
    let first = slide(&format!(
        "{}{}{}{}",
        // Body before title in the tree; the title must still come first.
        sp(
            r#"idx="1""#,
            2_000_000,
            &format!(
                "{}{}{}",
                para(0, "Point one"),
                para(1, "Detail"),
                para(0, "Point two")
            )
        ),
        sp(r#"type="title""#, 100, &para(0, "Intro")),
        sp(
            "",
            5_000_000,
            r#"<a:p><a:r><a:rPr b="1"/><a:t>Bold</a:t></a:r><a:r><a:rPr/><a:t xml:space="preserve"> and </a:t></a:r><a:r><a:rPr><a:hlinkClick r:id="rIdL"/></a:rPr><a:t>link</a:t></a:r></a:p>"#
        ),
        sp(r#"type="sldNum" idx="12""#, 6_000_000, &para(0, "1")),
    ));
    let table = r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="t"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="3000000"/><a:ext cx="1" cy="1"/></p:xfrm><a:graphic><a:graphicData uri="t"><a:tbl><a:tr h="1"><a:tc><a:txBody><a:p><a:r><a:t>Region</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>Sales</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr h="1"><a:tc><a:txBody><a:p><a:r><a:t>EU</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>12</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#;
    let second = slide(&format!(
        "{}{table}",
        sp(r#"type="title""#, 0, &para(0, "Numbers"))
    ));
    let notes = format!(
        r#"<p:notes {NS}><p:cSld><p:spTree>{}{}</p:spTree></p:cSld></p:notes>"#,
        sp(r#"type="sldImg""#, 0, ""),
        sp(
            r#"type="body" idx="1""#,
            0,
            &format!("{}{}", para(0, "Say hello"), para(0, "Then pause"))
        )
    );
    let presentation = format!(
        r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="257" r:id="rId3"/><p:sldId id="256" r:id="rId2"/></p:sldIdLst></p:presentation>"#
    );
    zip(&[
        ("_rels/.rels".into(), rels(&[("rId1", "officeDocument", "ppt/presentation.xml")])),
        ("ppt/presentation.xml".into(), presentation),
        ("ppt/_rels/presentation.xml.rels".into(), rels(&[("rId2", "slide", "slides/slide2.xml"), ("rId3", "slide", "slides/slide1.xml")])),
        ("ppt/slides/slide1.xml".into(), first),
        ("ppt/slides/_rels/slide1.xml.rels".into(), rels(&[("rIdL", "hyperlink", "https://example.com"), ("rIdN", "notesSlide", "../notesSlides/notesSlide1.xml")])),
        ("ppt/slides/slide2.xml".into(), second),
        ("ppt/notesSlides/notesSlide1.xml".into(), notes),
        ("docProps/core.xml".into(), r#"<cp:coreProperties xmlns:cp="c" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Deck</dc:title></cp:coreProperties>"#.into()),
    ])
}

#[test]
fn slides_in_presentation_order_with_titles_lists_links_and_notes() {
    let out = convert(&deck(), &Options::default()).unwrap();
    assert_eq!(out.title.as_deref(), Some("Deck"));
    let labels: Vec<&str> = out.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["slide 1", "slide 2"]);
    // sldIdLst lists slide1.xml first even though its relationship id sorts later.
    assert_eq!(
        out.sections[0].markdown,
        "## Intro\n\n- Point one\n  - Detail\n- Point two\n\n**Bold** and [link](https://example.com)\n\n> Notes: Say hello\n> Then pause\n"
    );
    assert_eq!(
        out.sections[1].markdown,
        "## Numbers\n\n|Region|Sales|\n|-|-|\n|EU|12|\n"
    );
}

#[test]
fn chart_series_render_as_table() {
    let chart = Element::default();
    assert_eq!(chart_markdown(&chart).unwrap(), "### Chart: untitled");
    let xml = br#"<c:chartSpace xmlns:c="c" xmlns:a="a"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Revenue</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>2025</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>12.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
    let root = ooxml::parse_xml(xml).unwrap();
    assert_eq!(
        chart_markdown(&root).unwrap(),
        "### Chart: Revenue\n\n|Category|2025|\n|-|-|\n|Q1|10|\n|Q2|12.5|"
    );
}

#[test]
fn malformed_input_is_invalid() {
    assert!(matches!(
        convert(b"PK\x03\x04garbage", &Options::default()),
        Err(ConvertError::Invalid(_))
    ));
    let empty = zip(&[("x".into(), "y".into())]);
    assert!(matches!(
        convert(&empty, &Options::default()),
        Err(ConvertError::Invalid(_))
    ));
}
