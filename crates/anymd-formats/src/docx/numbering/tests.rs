use super::{docx, md, p, r, W};

#[test]
fn numbered_and_bulleted_lists() {
    let numbering = format!(
        r#"<w:numbering {W}>
<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="2"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/></w:lvl></w:abstractNum>
<w:num w:numId="5"><w:abstractNumId w:val="1"/></w:num><w:num w:numId="6"><w:abstractNumId w:val="2"/></w:num>
</w:numbering>"#
    );
    let item = |num: u32, lvl: u32, text: &str| {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="{num}"/></w:numPr></w:pPr>{}</w:p>"#,
            r(text)
        )
    };
    let body = [
        item(5, 0, "apple"),
        item(5, 1, "green"),
        item(5, 0, "pear"),
        p("", &r("between")),
        item(6, 0, "one"),
        item(6, 1, "sub"),
        item(6, 1, "sub2"),
        item(6, 0, "two"),
    ]
    .concat();
    let bytes = docx(&body, &[("word/numbering.xml", &numbering)]);
    assert_eq!(
        md(&bytes),
        "- apple\n  - green\n- pear\n\nbetween\n\n1. one\n   1. sub\n   2. sub2\n2. two\n"
    );
}
