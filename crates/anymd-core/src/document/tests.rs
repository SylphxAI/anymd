use super::*;

fn pdf_fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(name),
    )
    .expect("read PDF fixture")
}

#[test]
fn pdf_magic_wins_over_hints_and_preserves_text_and_bookmarks() {
    let bytes = pdf_fixture("differential/v3014-behavior-v1.pdf");
    let pdf = open_bytes(
        &bytes,
        &OpenBytesOptions {
            filename: Some("report.csv".into()),
            content_type: Some("text/html".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(pdf.format, "pdf");
    assert_eq!(pdf.unit_noun, "page");
    assert!(pdf.paged);
    assert_eq!(pdf.title.as_deref(), Some("Parity Corpus V1"));
    assert_eq!(pdf.units.len(), 3);
    for (index, needle) in ["PAGE_ONE_ALPHA", "PAGE_TWO_BETA", "PAGE_THREE_GAMMA"]
        .into_iter()
        .enumerate()
    {
        let unit = &pdf.units[index];
        assert_eq!(unit.number, index as u32 + 1);
        assert!(unit.markdown.contains(needle), "{}", unit.markdown);
    }
    assert_eq!(
        pdf.native_outline,
        [
            (0, "External docs".into(), None),
            (1, "Page three".into(), Some(3)),
        ]
    );
    assert_eq!(
        pdf.outline("report").node_at(3, 0).path,
        "External docs > Page three"
    );
    assert!(pdf.scanned_pages.is_empty());
    assert!(pdf.render_markdown("report").starts_with(
        "---\nsource: report\ntitle: Parity Corpus V1\npages: 3\n---\n\n<!-- page 1 -->\n\n"
    ));
}

#[test]
fn image_scan_is_reported_without_ocr_or_a_rendered_page() {
    let bytes = pdf_fixture("scanned-page.pdf");
    let pdf = open_bytes(&bytes, &OpenBytesOptions::default()).unwrap();
    assert_eq!(pdf.scanned_pages, [1]);
    assert!(!pdf.units[0].markdown.chars().any(char::is_alphanumeric));
    assert!(pdf.units[1].markdown.contains("real text layer"));

    // Remove the second page's content to contrast a scan with an empty page.
    let mut fixture = lopdf::Document::load_mem(&bytes).unwrap();
    let page_two = fixture.get_pages()[&2];
    fixture
        .get_object_mut(page_two)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .remove(b"Contents");
    let mut scan_and_blank = Vec::new();
    fixture.save_to(&mut scan_and_blank).unwrap();
    let pdf = open_bytes(&scan_and_blank, &OpenBytesOptions::default()).unwrap();
    assert_eq!(pdf.scanned_pages, [1]);
    assert!(pdf.units[1].markdown.is_empty());
    assert!(pdf.render_markdown("").contains("<!-- These pages have little or no selectable text (scanned or image-only). The anymd CLI can OCR them with a local `tesseract`. -->"));
}

#[test]
fn applies_word_revision_modes_to_an_extensionless_attachment() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../anymd-formats/tests/fixtures/critic");
    let bytes = std::fs::read(fixtures.join("tracked-changes.docx")).unwrap();
    for (revisions, expected) in [
        (Revisions::Markup, "I really love {~~fonts~>font-styles~~}"),
        (Revisions::Accept, "I really love font-styles."),
        (Revisions::Reject, "I really love fonts."),
    ] {
        let doc = open_bytes(
            &bytes,
            &OpenBytesOptions {
                filename: None,
                content_type: Some(
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                        .into(),
                ),
                revisions,
            },
        )
        .unwrap();
        assert_eq!(doc.format, "docx");
        assert_eq!(doc.units.len(), 1);
        assert_eq!(doc.units[0].number, 1);
        assert!(
            doc.units[0].markdown.contains(expected),
            "{}",
            doc.units[0].markdown
        );
        assert_eq!(
            doc.units[0].markdown.contains("{~~"),
            revisions == Revisions::Markup
        );
    }
}

#[test]
fn specific_mime_overrides_default_text_but_generic_mime_does_not_hide_html_or_csv() {
    let csv = b"name,city\nAda,London\n";
    let doc = open_bytes(
        csv,
        &OpenBytesOptions {
            content_type: Some(" Text/CSV; charset=utf-8 ".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(doc.format, "csv");
    assert!(doc.units[0].markdown.contains("|Ada|London|"));
    let doc = open_bytes(
        csv,
        &OpenBytesOptions {
            filename: Some("table.csv".into()),
            content_type: Some("text/plain".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(doc.format, "csv");
    let doc = open_bytes(
        b"<!doctype html><html><h1>Heading</h1></html>",
        &OpenBytesOptions {
            content_type: Some("text/plain".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(doc.format, "html");
    assert_eq!(doc.title.as_deref(), Some("Heading"));
}

#[test]
fn distinguishes_unrecognized_binary_from_pdf_parse_and_conversion_errors() {
    assert!(matches!(
        open_bytes(&[0, 159, 146, 150, 0, 1], &OpenBytesOptions::default()),
        Err(OpenError::UnrecognizedFormat)
    ));
    assert!(matches!(
        open_bytes(b"%PDF-not a document", &OpenBytesOptions::default()),
        Err(OpenError::PdfParse(_))
    ));
    assert!(matches!(
        open_bytes(
            b"not an image",
            &OpenBytesOptions {
                content_type: Some("image/png".into()),
                ..Default::default()
            }
        ),
        Err(OpenError::Conversion {
            format: "image",
            ..
        })
    ));
}
