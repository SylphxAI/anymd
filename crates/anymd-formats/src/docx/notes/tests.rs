use super::super::convert_with_output_limit;
use super::{
    comments, del, docx, dr, end, ins, md, p, pm, r, reference, resolved, Options, Revisions, W,
};

#[test]
fn body_and_all_definition_parts_share_one_output_ceiling() {
    let body = p(
        "",
        &format!(
            "{}<w:r><w:footnoteReference w:id='1'/><w:endnoteReference w:id='2'/></w:r>{}",
            r(&"a".repeat(64)),
            reference("0")
        ),
    );
    let footnotes = format!(
        "<w:footnotes {W}><w:footnote w:id='1'>{}</w:footnote></w:footnotes>",
        p("", &r(&"f".repeat(40)))
    );
    let endnotes = format!(
        "<w:endnotes {W}><w:endnote w:id='2'>{}</w:endnote></w:endnotes>",
        p("", &r(&"e".repeat(40)))
    );
    let comments = comments(&[("0", "", "", &p("", &r(&"c".repeat(16))))]);
    let bytes = docx(
        &body,
        &[
            ("word/footnotes.xml", &footnotes),
            ("word/endnotes.xml", &endnotes),
            ("word/comments.xml", &comments),
        ],
    );
    let expected = format!(
        "{}[^1][^2]{{>>{}<<}}\n\n[^1]: {}\n[^2]: {}\n",
        "a".repeat(64),
        "c".repeat(16),
        "f".repeat(40),
        "e".repeat(40)
    );
    assert_eq!(md(&bytes), expected);
    assert_eq!(
        convert_with_output_limit(&bytes, &Options::default(), expected.len())
            .unwrap()
            .sections[0]
            .markdown,
        expected
    );
    assert_eq!(
        convert_with_output_limit(&bytes, &Options::default(), expected.len() - 1)
            .unwrap_err()
            .limit_exceeded(),
        Some(crate::ConversionLimit {
            resource: "output",
            limit: crate::limits::MAX_OUTPUT_BYTES
        })
    );
    // Body (including the comment) and definitions each fit independently;
    // the old two-budget implementation would retain their oversized sum.
    assert_eq!(
        convert_with_output_limit(&bytes, &Options::default(), 128)
            .unwrap_err()
            .limit_exceeded()
            .unwrap()
            .resource,
        "output"
    );
}

#[test]
fn a_large_body_cannot_get_a_second_budget_for_footnotes() {
    let body = p(
        "",
        &format!(
            "{}<w:r><w:footnoteReference w:id='1'/></w:r>",
            r(&"a".repeat(80))
        ),
    );
    let notes = format!(
        "<w:footnotes {W}><w:footnote w:id='1'>{}{}</w:footnote></w:footnotes>",
        p("", &r(&"b".repeat(40))),
        p("", &r(&"c".repeat(40)))
    );
    let bytes = docx(&body, &[("word/footnotes.xml", &notes)]);
    assert_eq!(
        convert_with_output_limit(&bytes, &Options::default(), 128)
            .unwrap_err()
            .limit_exceeded()
            .unwrap()
            .resource,
        "output"
    );
    let bytes = docx(&p("", &r(&"a".repeat(80))), &[]);
    assert!(convert_with_output_limit(&bytes, &Options::default(), 128).is_ok());
}

#[test]
fn review_footnote_revisions_are_marked() {
    let body = p(
        "",
        r#"<w:r><w:t>Claim</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r>"#,
    );
    let footnotes = format!(
        r#"<w:footnotes {W}><w:footnote w:id="2">{}{}</w:footnote></w:footnotes>"#,
        pm(
            "",
            &["del"],
            &format!("{}{}", r("Source"), ins(&r(" updated")))
        ),
        p("", &r("page 4")),
    );
    assert_eq!(
        md(&docx(&body, &[("word/footnotes.xml", &footnotes)])),
        "Claim[^1]\n\n[^1]: Source{++ updated++}{>>Ana (2026-01-02T03:04:00Z)<<}{-- --}{>>Ana (2026-01-02T03:04:00Z)<<}page 4\n"
    );
}

#[test]
fn a_note_whose_reference_is_changed_is_changed_too() {
    let reference = |id: &str| format!(r#"<w:r><w:footnoteReference w:id="{id}"/></w:r>"#);
    let body = [
        p("", &(r("Kept") + &reference("2"))),
        p("", &ins(&(r("Added") + &reference("3")))),
        p("", &del(&(dr("Gone") + &reference("4")))),
    ]
    .concat();
    let footnotes = format!(
        r#"<w:footnotes {W}><w:footnote w:id="2">{}</w:footnote><w:footnote w:id="3">{}</w:footnote><w:footnote w:id="4">{}</w:footnote></w:footnotes>"#,
        p("", &r("Plain note.")),
        p("", &r("Added note.")),
        p("", &format!("{}{}", r("Gone "), ins(&r("note")))),
    );
    let bytes = docx(&body, &[("word/footnotes.xml", &footnotes)]);
    let by = "{>>Ana (2026-01-02T03:04:00Z)<<}";
    assert_eq!(
        md(&bytes),
        format!(
            "Kept[^1]\n\n{{++Added[^2]++}}{by}\n\n{{--Gone[^3]--}}{by}\n\n[^1]: Plain note.\n{{++[^2]: Added note.++}}{by}\n{{--[^3]: Gone {{++note++}}{by}--}}{by}\n"
        )
    );
    // Resolved first, the notes are numbered as they are left.
    assert_eq!(
        resolved(&bytes, Revisions::Reject),
        "Kept[^1]\n\nGone[^2]\n\n[^1]: Plain note.\n[^2]: Gone\n"
    );
}
