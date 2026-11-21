use super::{convert, docx, p, r, Options};

#[test]
fn headings_emphasis_links_and_core_title() {
    let body = [
        p("Title", &r("Report")),
        p("Heading2", r#"<w:r><w:rPr><w:b/></w:rPr><w:t>Scope</w:t></w:r>"#),
        p(
            "",
            &format!(
                r#"{}<w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">bold </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>run</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t xml:space="preserve"> it</w:t></w:r><w:r><w:t>, see </w:t></w:r><w:hyperlink r:id="rId9"><w:r><w:t>docs</w:t></w:r></w:hyperlink><w:r><w:t>.</w:t></w:r>"#,
                r("Plain ")
            ),
        ),
        r#"<w:p><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>Outline</w:t></w:r></w:p>"#.to_string(),
        r#"<w:p><w:r><w:t>a</w:t></w:r><w:r><w:br/><w:t>b</w:t><w:tab/><w:t>c</w:t></w:r></w:p>"#.to_string(),
        "<w:p/>".to_string(),
    ]
    .concat();
    let bytes = docx(
        &body,
        &[
            (
                "word/_rels/document.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a b" TargetMode="External"/></Relationships>"#,
            ),
            (
                "docProps/core.xml",
                r#"<cp:coreProperties xmlns:cp="x" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Quarterly</dc:title><dc:creator>Ada</dc:creator></cp:coreProperties>"#,
            ),
        ],
    );
    let out = convert(&bytes, &Options::default()).unwrap();
    assert_eq!(out.title.as_deref(), Some("Quarterly"));
    assert_eq!(
        out.metadata,
        vec![("author".to_string(), "Ada".to_string())]
    );
    assert_eq!(
        out.sections[0].markdown,
        "# Report\n\n## Scope\n\nPlain **bold run** _it_, see [docs](https://example.com/a%20b).\n\n### Outline\n\na\nb\tc\n"
    );
}
