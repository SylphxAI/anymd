use super::{docx, md, p, r};

#[test]
fn tables_with_spans_and_nested_tables() {
    let tc = |content: &str| format!("<w:tc>{content}</w:tc>");
    let nested = format!(
        "<w:tbl><w:tr>{}{}</w:tr></w:tbl>",
        tc(&p("", &r("n1"))),
        tc(&p("", &r("n2")))
    );
    let body = format!(
        r#"<w:tbl><w:tr>{}{}{}</w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr>{}</w:tc>{}</w:tr><w:tr>{}{}{}</w:tr></w:tbl>"#,
        tc(&p("", &r("Name"))),
        tc(&p("", &r("Q1"))),
        tc(&p("", &r("Q2"))),
        p("", &r("wide|cell")),
        tc(&p("", &r("x"))),
        tc(&format!("{}{}", p("", &r("line1")), p("", &r("line2")))),
        tc(&nested),
        tc(""),
    );
    assert_eq!(
        md(&docx(&body, &[])),
        "|Name|Q1|Q2|\n|-|-|-|\n|wide\\|cell||x|\n|line1<br>line2|n1; n2||\n"
    );
}
