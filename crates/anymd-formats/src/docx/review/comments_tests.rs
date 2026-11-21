use super::super::review::comment_note;
use super::{
    by, comments, del, docx, dr, end, ins, md, p, pm, r, reference, resolved, text_box, Revisions,
    W,
};

fn start(id: &str) -> String {
    format!(r#"<w:commentRangeStart w:id="{id}"/>"#)
}

fn with_comments(body: &str, xml: &str) -> String {
    md(&docx(body, &[("word/comments.xml", xml)]))
}

const BILL: (&str, &str) = ("Bill Winter", "2024-04-08T10:32:00Z");

#[test]
fn comment_highlights_its_range_and_names_author_and_date_as_word_stores_them() {
    let body = p(
        "",
        &format!(
            "{}{}{}{}{}{}",
            r("Truth is "),
            start("0"),
            r("stranger than fiction"),
            end("0"),
            reference("0"),
            r(".")
        ),
    );
    let xml = comments(&[("0", BILL.0, BILL.1, &p("", &r("true")))]);
    assert_eq!(
        with_comments(&body, &xml),
        "Truth is {==stranger than fiction==}{>>Bill Winter (2024-04-08T10:32:00Z): true<<}.\n"
    );
}

#[test]
fn comment_date_is_copied_verbatim_whatever_its_form() {
    // Fractional seconds and offsets are valid xsd:dateTime; they are kept.
    for date in [
        "2024-04-08T10:32:17.123Z",
        "2024-04-08T10:32:00+02:00",
        "2024-04-08T10:32:00",
    ] {
        let body = p("", &format!("{}{}", r("a"), reference("1")));
        let xml = comments(&[("1", "Ana", date, &p("", &r("n")))]);
        assert_eq!(
            with_comments(&body, &xml),
            format!("a{{>>Ana ({date}): n<<}}\n")
        );
    }
}

#[test]
fn comment_note_leaves_out_what_is_missing() {
    assert_eq!(
        comment_note(Some("Ana"), Some("2026-01-02T03:04:00Z"), "hi"),
        "Ana (2026-01-02T03:04:00Z): hi"
    );
    assert_eq!(comment_note(Some("Ana"), None, "hi"), "Ana: hi");
    assert_eq!(
        comment_note(None, Some("2026-01-02T03:04:00Z"), "hi"),
        "(2026-01-02T03:04:00Z): hi"
    );
    assert_eq!(comment_note(Some("  "), Some(" "), "hi"), "hi");
    assert_eq!(
        comment_note(Some("Ana"), Some("2026-01-02T03:04:00Z"), ""),
        "Ana (2026-01-02T03:04:00Z)"
    );
    assert_eq!(comment_note(None, None, ""), "");
    assert_eq!(comment_note(Some("A <<} B"), None, "x"), "A <<\\} B: x");
}

#[test]
fn comment_without_a_range_is_a_plain_comment_at_its_reference() {
    let body = p(
        "",
        &format!(
            "{}{}{}",
            r("Lorem ipsum dolor sit amet."),
            reference("3"),
            r(" Next")
        ),
    );
    let xml = comments(&[("3", "Ana", "", &p("", &r("This is a comment")))]);
    assert_eq!(
        with_comments(&body, &xml),
        "Lorem ipsum dolor sit amet.{>>Ana: This is a comment<<} Next\n"
    );
}

#[test]
fn replies_and_overlapping_ranges_share_one_highlight() {
    // Word writes a reply as its own comment over the same range.
    let body = p(
        "",
        &[
            start("0"),
            r("a"),
            start("1"),
            start("2"),
            r("b"),
            end("0"),
            reference("0"),
            r("c"),
            end("1"),
            reference("1"),
            end("2"),
            reference("2"),
        ]
        .concat(),
    );
    let xml = comments(&[
        ("0", "Ana", "2026-01-01T09:00:00Z", &p("", &r("first"))),
        ("1", "Bo", "2026-01-01T10:00:00Z", &p("", &r("second"))),
        ("2", "Ana", "2026-01-01T11:00:00Z", &p("", &r("reply"))),
    ]);
    assert_eq!(
        with_comments(&body, &xml),
        "{==abc==}{>>Ana (2026-01-01T09:00:00Z): first<<}{>>Bo (2026-01-01T10:00:00Z): second<<}{>>Ana (2026-01-01T11:00:00Z): reply<<}\n"
    );
}

#[test]
fn comment_range_across_paragraphs_highlights_each_paragraph() {
    let body = [
        p("", &format!("{}{}{}", r("one "), start("5"), r("two"))),
        p("Heading1", &r("three")),
        p(
            "",
            &format!("{}{}{}{}", r("four"), end("5"), reference("5"), r(" five")),
        ),
    ]
    .concat();
    let xml = comments(&[("5", "Ana", "", &p("", &r("long")))]);
    assert_eq!(
        with_comments(&body, &xml),
        "one {==two==}\n\n# {==three==}\n\n{==four==}{>>Ana: long<<} five\n"
    );
}

#[test]
fn comment_ranges_at_block_level_and_in_tables() {
    let body = format!(
        "{}{}<w:tbl><w:tr><w:tc>{}{}</w:tc></w:tr></w:tbl>{}",
        start("1"),
        p("", &r("para")),
        p("", &r("cell")),
        end("1"),
        p("", &format!("{}{}", r("after"), reference("1"))),
    );
    let xml = comments(&[("1", "Ana", "", &p("", &r("n")))]);
    assert_eq!(
        with_comments(&body, &xml),
        "{==para==}\n\n|{==cell==}|\n|-|\n\nafter{>>Ana: n<<}\n"
    );
}

#[test]
fn comment_without_a_reference_gets_its_note_at_the_range_end() {
    let body = p(
        "",
        &format!("{}{}{}{}", start("4"), r("x"), end("4"), r("y")),
    );
    let xml = comments(&[("4", "Ana", "", &p("", &r("n")))]);
    assert_eq!(with_comments(&body, &xml), "{==x==}{>>Ana: n<<}y\n");
}

#[test]
fn unknown_or_malformed_comments_are_ignored() {
    let body = p(
        "",
        &format!(
            "{}{}{}{}{}{}{}",
            start("9"),
            r("a"),
            end("9"),
            reference("9"),
            end("8"),
            r("b"),
            r#"<w:commentRangeStart/><w:r><w:commentReference/></w:r>"#,
        ),
    );
    // Without a comments part every anchor is ignored.
    assert_eq!(md(&docx(&body, &[])), "ab\n");
    // A comment without an id cannot be referenced; one never closed stays a
    // highlight to the end of its paragraph only.
    let xml = format!(
        r#"<w:comments {W}><w:comment w:author="Ana">{}</w:comment><w:comment w:id="8" w:author="Ana">{}</w:comment></w:comments>"#,
        p("", &r("x")),
        p("", &r("y"))
    );
    let open = [
        p("", &format!("{}{}", start("8"), r("open"))),
        p("", &r("still")),
    ]
    .concat();
    assert_eq!(with_comments(&body, &xml), "ab\n");
    assert_eq!(with_comments(&open, &xml), "{==open==}\n\n{==still==}\n");
    // A range start seen twice opens once.
    let twice = p(
        "",
        &format!(
            "{}{}{}{}{}",
            start("8"),
            start("8"),
            r("x"),
            end("8"),
            reference("8")
        ),
    );
    assert_eq!(with_comments(&twice, &xml), "{==x==}{>>Ana: y<<}\n");
}

#[test]
fn comment_text_keeps_formatting_joins_paragraphs_and_escapes_delimiters() {
    let body = p("", &format!("{}{}", r("a"), reference("1")));
    let text = [
        p("", r#"<w:r><w:rPr><w:b/></w:rPr><w:t>Bold</w:t></w:r><w:r><w:br/><w:t>line &lt;&lt;} end</w:t></w:r>"#),
        p("", ""),
        pm("", &["del"], &r("gone")),
        p("", &format!("{}{}", r("kept"), ins(&r(" new")))),
    ]
    .concat();
    let xml = comments(&[("1", "Ana", "", &text)]);
    assert_eq!(
        with_comments(&body, &xml),
        "a{>>Ana: **Bold** line <<\\} end gone{-- --}kept{++ new++}<<}\n"
    );
}

#[test]
fn comments_meet_tracked_changes_without_breaking_either() {
    // Word wraps the reference run of a comment added with tracking on in
    // w:ins; the note must not become an insertion.
    let tracked = p(
        "",
        &format!(
            "{}{}{}{}",
            start("1"),
            r("text"),
            end("1"),
            ins(&reference("1"))
        ),
    );
    // A comment on deleted text, and a range that ends inside an insertion.
    let deleted = p(
        "",
        &format!(
            "{}{}{}{}",
            start("1"),
            del(&dr("old")),
            end("1"),
            reference("1")
        ),
    );
    let crossing = p(
        "",
        &format!(
            "{}{}{}{}",
            start("1"),
            r("a"),
            ins(&format!("{}{}{}", r("b"), end("1"), r("c"))),
            reference("1")
        ),
    );
    let xml = comments(&[("1", "Ana", "", &p("", &r("n")))]);
    assert_eq!(with_comments(&tracked, &xml), "{==text==}{>>Ana: n<<}\n");
    assert_eq!(
        with_comments(&deleted, &xml),
        "{=={--old--}{>>Ana (2026-01-02T03:04:00Z)<<}==}{>>Ana: n<<}\n"
    );
    assert_eq!(
        with_comments(&crossing, &xml),
        "{==a{++b++}{>>Ana (2026-01-02T03:04:00Z)<<}==}{++c++}{>>Ana (2026-01-02T03:04:00Z)<<}{>>Ana: n<<}\n"
    );
}

#[test]
fn comment_range_around_a_text_box_does_not_cover_the_box() {
    let body = p(
        "",
        &format!(
            "{}{}{}{}{}",
            start("1"),
            r("anchor"),
            text_box(&p("", &r("inside"))),
            end("1"),
            reference("1")
        ),
    );
    let xml = comments(&[("1", "Ana", "", &p("", &r("n")))]);
    assert_eq!(
        with_comments(&body, &xml),
        "{==anchor==}{>>Ana: n<<}\n\ninside\n"
    );
}

#[test]
fn comment_part_is_found_through_the_document_relationships() {
    let body = p("", &format!("{}{}", r("a"), reference("1")));
    let xml = comments(&[("1", "Ana", "", &p("", &r("n")))]);
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="notes/remarks.xml"/></Relationships>"#;
    let bytes = docx(
        &body,
        &[
            ("word/_rels/document.xml.rels", rels),
            ("word/notes/remarks.xml", &xml),
        ],
    );
    assert_eq!(md(&bytes), "a{>>Ana: n<<}\n");
}

#[test]
fn comments_in_footnotes_and_ranges_left_open_before_them() {
    let body = p(
        "",
        &format!(
            "{}{}{}",
            start("1"),
            r("Claim"),
            r#"<w:r><w:footnoteReference w:id="2"/></w:r>"#
        ),
    );
    let footnotes = format!(
        r#"<w:footnotes {W}><w:footnote w:id="2">{}</w:footnote></w:footnotes>"#,
        p(
            "",
            &format!(
                "{}{}{}{}",
                start("2"),
                r("Source"),
                end("2"),
                reference("2")
            )
        ),
    );
    let xml = comments(&[
        ("1", "Ana", "", &p("", &r("never closed"))),
        ("2", "Bo", "", &p("", &r("check"))),
    ]);
    assert_eq!(
        md(&docx(
            &body,
            &[
                ("word/comments.xml", &xml),
                ("word/footnotes.xml", &footnotes)
            ]
        )),
        "{==Claim[^1]==}\n\n[^1]: {==Source==}{>>Bo: check<<}\n"
    );
}

#[test]
fn comment_without_an_author_shows_only_its_date() {
    let body = p("", &format!("{}{}", r("a"), reference("1")));
    let xml = comments(&[("1", "", "2026-01-02T03:04:00Z", &p("", &r("anonymous")))]);
    assert_eq!(
        with_comments(&body, &xml),
        "a{>>(2026-01-02T03:04:00Z): anonymous<<}\n"
    );
}

#[test]
fn footnotes_in_comment_text_are_numbered_where_the_comment_is() {
    let body = [
        p(
            "",
            r#"<w:r><w:t>B</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r>"#,
        ),
        p("", &format!("{}{}", r("A"), reference("1"))),
    ]
    .concat();
    let footnotes = format!(
        r#"<w:footnotes {W}><w:footnote w:id="2">{}</w:footnote><w:footnote w:id="3">{}</w:footnote></w:footnotes>"#,
        p("", &r("body note")),
        p("", &r("comment note")),
    );
    let xml = comments(&[(
        "1",
        "Ana",
        "",
        &p(
            "",
            r#"<w:r><w:t>see</w:t></w:r><w:r><w:footnoteReference w:id="3"/></w:r>"#,
        ),
    )]);
    assert_eq!(
        md(&docx(
            &body,
            &[
                ("word/comments.xml", &xml),
                ("word/footnotes.xml", &footnotes)
            ]
        )),
        "B[^1]\n\nA{>>Ana: see[^2]<<}\n\n[^1]: body note\n[^2]: comment note\n"
    );
}

#[test]
fn a_range_ending_between_paragraphs_keeps_its_note() {
    let xml = comments(&[
        ("1", "Ana", "", &p("", &r("first"))),
        ("2", "Bo", "", &p("", &r("last"))),
    ]);
    let body = [
        p("", &format!("{}{}", start("1"), r("a"))),
        end("1"),
        p("", &r("b")),
        p("", &format!("{}{}", start("2"), r("c"))),
        end("2"),
    ]
    .concat();
    assert_eq!(
        with_comments(&body, &xml),
        "{==a==}\n\n{>>Ana: first<<}b\n\n{==c==}\n\n{>>Bo: last<<}\n"
    );
}

#[test]
fn a_note_between_paragraph_and_table_stays_out_of_the_table() {
    let xml = comments(&[("1", "Ana", "", &p("", &r("first")))]);
    let table = |cell: &str| format!("<w:tbl><w:tr><w:tc>{cell}</w:tc></w:tr></w:tbl>");
    let body = [
        p("", &format!("{}{}", start("1"), r("a"))),
        end("1"),
        table(&p("", &r("cell"))),
    ]
    .concat();
    assert_eq!(
        with_comments(&body, &xml),
        "{==a==}\n\n{>>Ana: first<<}\n\n|cell|\n|-|\n"
    );
    // A deleted paragraph mark before the note keeps its markup.
    let body = [
        pm("", &["del"], &format!("{}{}", start("1"), r("a"))),
        end("1"),
        table(&p("", &r("cell"))),
    ]
    .concat();
    assert_eq!(
        with_comments(&body, &xml),
        "{==a==}{--\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}{>>Ana: first<<}\n\n|cell|\n|-|\n"
    );
}

#[test]
fn comment_text_keeps_text_boxes_without_nesting_comments() {
    let inner = [
        p(
            "",
            &by("ins", "Bo Chen", "2026-09-29T15:10:00Z", &r("boxed")),
        ),
        p("", &format!("{}{}", r("with a note"), reference("2"))),
    ]
    .concat();
    let text = p(
        "",
        &format!("{}{}{}", r("see "), text_box(&inner), reference("2")),
    );
    let xml = comments(&[
        ("1", "Ana", "", &text),
        ("2", "Bo", "", &p("", &r("inner"))),
    ]);
    let body = p(
        "",
        &format!("{}{}{}{}", start("1"), r("x"), end("1"), reference("1")),
    );
    assert_eq!(
        with_comments(&body, &xml),
        "{==x==}{>>Ana: see {++boxed++} with a note<<}\n"
    );
}

#[test]
fn a_comment_used_twice_is_written_once_and_its_footnote_counted_once() {
    let body = [
        p("", &format!("{}{}", r("A"), reference("1"))),
        p("", &format!("{}{}", r("B"), reference("1"))),
    ]
    .concat();
    let footnotes = format!(
        r#"<w:footnotes {W}><w:footnote w:id="3">{}</w:footnote></w:footnotes>"#,
        p("", &r("source")),
    );
    let xml = comments(&[(
        "1",
        "Ana",
        "",
        &p(
            "",
            r#"<w:r><w:t>see</w:t></w:r><w:r><w:footnoteReference w:id="3"/></w:r>"#,
        ),
    )]);
    assert_eq!(
        md(&docx(
            &body,
            &[
                ("word/comments.xml", &xml),
                ("word/footnotes.xml", &footnotes)
            ]
        )),
        "A{>>Ana: see[^1]<<}\n\nB{>>Ana: see[^1]<<}\n\n[^1]: source\n"
    );
}

#[test]
fn accept_and_reject_leave_comments_out() {
    let body = p(
        "",
        &format!("{}{}{}{}", start("1"), r("noted"), end("1"), reference("1")),
    );
    let xml = comments(&[("1", "Ana", "", &p("", &r("why?")))]);
    let bytes = docx(&body, &[("word/comments.xml", &xml)]);
    assert!(md(&bytes).contains("{==noted==}"), "{}", md(&bytes));
    assert_eq!(resolved(&bytes, Revisions::Accept), "noted\n");
    assert_eq!(resolved(&bytes, Revisions::Reject), "noted\n");
}
