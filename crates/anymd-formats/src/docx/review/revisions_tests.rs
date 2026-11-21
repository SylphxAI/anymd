use super::{by, del, docx, dr, ins, md, p, pm, r, resolved, text_box, Revisions, W};

#[test]
fn tracked_changes_become_critic_markup_without_losing_formatting() {
    let body = [
        p(
            "",
            r#"<w:r><w:t xml:space="preserve">Keep </w:t></w:r><w:del w:id="1"><w:r><w:rPr><w:i/></w:rPr><w:delText>old</w:delText></w:r></w:del><w:ins w:id="2"><w:r><w:rPr><w:b/></w:rPr><w:t>new</w:t></w:r></w:ins><w:r><w:t>.</w:t></w:r>"#,
        ),
        p(
            "",
            r#"<w:moveTo w:id="3"><w:r><w:t>destination</w:t></w:r></w:moveTo><w:r><w:t xml:space="preserve"> / </w:t></w:r><w:moveFrom w:id="3"><w:r><w:delText>source</w:delText></w:r></w:moveFrom>"#,
        ),
    ]
    .concat();

    // A deletion next to an insertion is a replacement: CriticMarkup's
    // substitution, as pandiff writes it.
    assert_eq!(
        md(&docx(&body, &[])),
        "Keep {~~_old_~>**new**~~}.\n\n{++destination++} / {--source--}\n"
    );
}

#[test]
fn tracked_whole_paragraphs_keep_markdown_block_structure() {
    let body = format!(
        r#"<w:ins w:id="1">{}</w:ins><w:del w:id="2">{}</w:del>"#,
        p("Heading2", &r("Added heading")),
        p("", &r("Deleted paragraph")),
    );

    assert_eq!(
        md(&docx(&body, &[])),
        "## {++Added heading++}\n\n{--Deleted paragraph--}\n"
    );
}

#[test]
fn tracked_table_rows_and_cells_keep_the_table_valid() {
    let body = format!(
        r#"<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}</w:tc></w:tr><w:tr><w:trPr><w:del/></w:trPr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellIns/></w:tcPr>{}</w:tc></w:tr></w:tbl>"#,
        p("", &r("Before")),
        p("", &r("After")),
        p("", &r("old")),
        p("", &r("new")),
    );

    // The inserted cell sits in a deleted row: accepting removes the row and
    // rejecting removes the cell, so the text is gone either way.
    assert_eq!(
        md(&docx(&body, &[])),
        "|Before|After|\n|-|-|\n|{--old--}|{--{++new++}--}|\n"
    );
}

#[test]
fn review_deleted_row_with_deleted_runs_is_marked_once() {
    // Word deletes a row by marking the row, every run and every paragraph mark.
    let body = format!(
        r#"<w:tbl><w:tr><w:tc>{}</w:tc></w:tr><w:tr><w:trPr><w:del w:id="1" w:author="Ana" w:date="2026-01-02T03:04:00Z"/></w:trPr><w:tc>{}</w:tc></w:tr></w:tbl>"#,
        p("", &r("Head")),
        pm("", &["del"], &del(&dr("old"))),
    );
    assert_eq!(
        md(&docx(&body, &[])),
        "|Head|\n|-|\n|{--old--}{>>Ana (2026-01-02T03:04:00Z)<<}|\n"
    );
}

#[test]
fn review_text_box_inside_a_tracked_change_keeps_the_change() {
    let body = [
        p("", &del(&text_box(&p("", &r("Deleted box"))))),
        p(
            "",
            &format!("{}{}", r("Keep"), ins(&text_box(&p("", &r("Added box"))))),
        ),
    ]
    .concat();
    // The deleted box leaves no empty `{----}` paragraph behind.
    assert_eq!(
        md(&docx(&body, &[])),
        "{--Deleted box--}{>>Ana (2026-01-02T03:04:00Z)<<}\n\nKeep\n\n{++Added box++}{>>Ana (2026-01-02T03:04:00Z)<<}\n"
    );
}

#[test]
fn review_tracked_paragraph_marks_mark_the_break() {
    let deleted = [pm("", &["del"], &r("A")), p("", &r("B"))].concat();
    assert_eq!(
        md(&docx(&deleted, &[])),
        "A{--\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}B\n"
    );
    let inserted = [pm("", &["ins"], &r("A")), p("", &r("B"))].concat();
    assert_eq!(
        md(&docx(&inserted, &[])),
        "A{++\n\n++}{>>Ana (2026-01-02T03:04:00Z)<<}B\n"
    );
    // A whole deleted paragraph: its text and its break are one deletion.
    let whole = [
        pm("", &["del"], &del(&dr("Deleted paragraph"))),
        p("", &r("Next")),
    ]
    .concat();
    assert_eq!(
        md(&docx(&whole, &[])),
        "{--Deleted paragraph\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}Next\n"
    );
}

#[test]
fn substitutions_merges_and_empty_changes() {
    let body = [
        // Replacement written insertion first still reads old ~> new.
        p("", &format!("{}{}{}", r("I really love "), ins(&r("font-styles")), del(&dr("fonts")))),
        // Word splits one insertion over several w:ins; they join.
        p("", &format!("{}{}{}", ins(&r("one ")), ins(&r("insertion")), r("."))),
        // Empty change elements vanish; an inserted space stays.
        p("", &format!("{}{}{}{}", r("a"), ins(""), del(&r("")), ins(&r(" ")))),
        // Inserted by one author and deleted by another.
        p("", &format!("{}{}", r("x"), ins(&del(&dr("y"))))),
        // Moves are an insertion and a deletion; next to each other they read
        // as a substitution.
        p("", r#"<w:moveFrom w:id="3"><w:r><w:delText>here</w:delText></w:r></w:moveFrom><w:moveTo w:id="4"><w:r><w:t>there</w:t></w:r></w:moveTo>"#),
    ]
    .concat();
    assert_eq!(
        md(&docx(&body, &[])),
        "I really love {~~fonts~>font-styles~~}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{++one insertion++}{>>Ana (2026-01-02T03:04:00Z)<<}.\n\na{++ ++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\nx{++{--y--}{>>Ana (2026-01-02T03:04:00Z)<<}++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{~~here~>there~~}\n"
    );
}

#[test]
fn delimiters_in_document_text_do_not_open_spans() {
    // With a tracked change in the document, delimiter-like text is escaped.
    let body = p("", &r("x {++ y ++} ~> z")) + &p("", &ins(&r("new")));
    assert_eq!(
        md(&docx(&body, &[])),
        "x {\\++ y ++\\} ~\\> z\n\n{++new++}{>>Ana (2026-01-02T03:04:00Z)<<}\n"
    );
    // Without one, nothing is marked up, so the text is kept as is.
    let body = p("", &r("x {++ y ++} ~> z"));
    assert_eq!(md(&docx(&body, &[])), "x {++ y ++} ~> z\n");
}

#[test]
fn tracked_changes_with_links_fields_and_hidden_runs() {
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://x.test" TargetMode="External"/></Relationships>"#;
    let body = [
        p("", &format!(r#"<w:hyperlink r:id="rId9">{}{}</w:hyperlink>"#, r("keep "), ins(&r("new")))),
        p("", &ins(r#"<w:hyperlink r:id="rId9"><w:r><w:t>added link</w:t></w:r></w:hyperlink>"#)),
        // A deleted hyperlink field keeps its target through w:delInstrText.
        p("", &del(r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:delInstrText> HYPERLINK "https://f.test" </w:delInstrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:delText>old link</w:delText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#)),
        // Hidden text is not shown in Word, tracked or not.
        p("", &format!("{}{}", r("seen"), ins(r#"<w:r><w:rPr><w:vanish/></w:rPr><w:t>hidden</w:t></w:r>"#))),
    ]
    .concat();
    assert_eq!(
        md(&docx(&body, &[("word/_rels/document.xml.rels", rels)])),
        "[keep](https://x.test) {++[new](https://x.test)++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{++[added link](https://x.test)++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{--[old link](https://f.test)--}{>>Ana (2026-01-02T03:04:00Z)<<}\n\nseen\n"
    );
}

#[test]
fn tracked_breaks_keep_headings_lists_and_blocks_valid() {
    let numbering = format!(
        r#"<w:numbering {W}><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="5"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
    );
    let item = |marks: &[&str], runs: &str| {
        let marks: String = marks.iter().map(|m| format!("<w:{m}/>")).collect();
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="5"/></w:numPr><w:rPr>{marks}</w:rPr></w:pPr>{runs}</w:p>"#
        )
    };
    let body = [
        // Before a heading the break closes after `## `.
        pm("", &["ins"], &r("A")),
        p("Heading2", &r("B")),
        // Before a list item it closes after the marker; a deleted item keeps
        // its number, as Word shows it.
        item(&["moveFrom"], &r("one")),
        item(&[], &del(&dr("two"))),
        item(&["moveTo"], &r("three")),
        item(&[], &r("four")),
        // A blank paragraph keeps its own break; a table cannot join a paragraph.
        pm("", &["del"], &r("C")),
        p("", ""),
        pm("", &["ins", "del"], &r("D")),
        p("", &r("E")),
        pm("", &["del"], &r("F")),
        format!(
            "<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
            p("", &r("G"))
        ),
        // A paragraph holding a text box gives its break to the box.
        pm(
            "",
            &["del"],
            &format!("{}{}", r("H"), text_box(&p("", &r("box")))),
        ),
        p("", &r("I")),
        // The last paragraph's mark has no break after it.
        pm("", &["del"], &r("J")),
    ]
    .concat();
    assert_eq!(
        md(&docx(&body, &[("word/numbering.xml", &numbering)])),
        "A{++\n\n## ++}{>>Ana (2026-01-02T03:04:00Z)<<}B\n\n1. one{--\n2. two--}{>>Ana (2026-01-02T03:04:00Z)<<}\n3. three{++\n4. ++}four\n\nC\n\nD{--\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}E\n\nF\n\n|G|\n|-|\n\nH\n\nbox\n\nI\n\nJ\n"
    );
}

#[test]
fn tracked_cells_rows_and_nested_tables() {
    let body = format!(
        r#"<w:tbl><w:tr><w:tc>{}{}{}</w:tc><w:tc>{}</w:tc></w:tr><w:tr><w:trPr><w:ins/></w:trPr><w:tc><w:tbl><w:tr><w:trPr><w:del/></w:trPr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellDel/></w:tcPr>{}</w:tc></w:tr></w:tbl></w:tc><w:tc>{}</w:tc></w:tr></w:tbl>"#,
        // Paragraph marks inside a cell.
        pm("", &["del"], &r("a")),
        pm("", &["ins"], &r("b")),
        pm("", &["del"], &r("c")),
        ins(&p("", &r("block ins"))),
        p("", &r("n1")),
        p("", &r("n2")),
        del(&p("", &r("wrapped"))),
    );
    assert_eq!(
        md(&docx(&body, &[])),
        "|a{--<br>--}{>>Ana (2026-01-02T03:04:00Z)<<}b{++<br>++}{>>Ana (2026-01-02T03:04:00Z)<<}c|{++block ins++}{>>Ana (2026-01-02T03:04:00Z)<<}|\n|-|-|\n|{++{--n1--}; {--n2--}++}|{++{--wrapped--}{>>Ana (2026-01-02T03:04:00Z)<<}++}|\n"
    );
}

#[test]
fn tracked_blocks_inside_content_controls() {
    let body = ins(&format!(
        "<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt><w:customXml>{}</w:customXml>",
        p("", &r("in control")),
        p("", &r("in custom xml")),
    ));
    assert_eq!(
        md(&docx(&body, &[])),
        "{++in control++}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n{++in custom xml++}{>>Ana (2026-01-02T03:04:00Z)<<}\n"
    );
}

#[test]
fn tracked_changes_inside_inline_wrappers_and_styled_breaks() {
    let body = [
        // Smart tags and inline content controls wrap tracked runs too.
        p(
            "",
            &format!(
                "<w:smartTag>{}</w:smartTag><w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt>",
                ins(&r("tagged")),
                del(&dr(" boxed"))
            ),
        ),
        // A heading whose own mark is deleted joins the next paragraph.
        pm("Heading1", &["del"], &r("Title")),
        p("", &r("Body")),
        // A note reference without an id has nothing to point at.
        p(
            "",
            &format!("{}{}", r("x"), ins(r#"<w:r><w:footnoteReference/></w:r>"#)),
        ),
    ]
    .concat();
    assert_eq!(
        md(&docx(&body, &[])),
        "{~~ boxed~>tagged~~}{>>Ana (2026-01-02T03:04:00Z)<<}\n\n# Title{--\n\n--}{>>Ana (2026-01-02T03:04:00Z)<<}Body\n\nx\n"
    );
}

#[test]
fn text_boxes_and_content_controls_inside_tracked_cells() {
    let body = format!(
        r#"<w:tbl><w:tr><w:trPr><w:del/></w:trPr><w:tc>{}</w:tc><w:tc><w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt></w:tc></w:tr></w:tbl>"#,
        // The box's own paragraph mark cannot join the box text.
        pm(
            "",
            &["del"],
            &format!("{}{}", r("cell"), text_box(&p("", &r("box"))))
        ),
        p("", &r("control")),
    );
    assert_eq!(
        md(&docx(&body, &[])),
        "|{--cell<br>box--}|{--control--}|\n|-|-|\n"
    );
}

#[test]
fn deleted_paragraph_mark_preserves_text_and_marks_only_the_separator() {
    let body = r#"<w:p><w:pPr><w:rPr><w:del w:id="1"/></w:rPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
<w:p><w:r><w:t>Second</w:t></w:r></w:p>
<w:p><w:r><w:t>Third</w:t></w:r></w:p>"#;
    assert_eq!(md(&docx(body, &[])), "First{--\n\n--}Second\n\nThird\n");
}

#[test]
fn deleted_text_boxes_keep_block_structure_without_an_empty_paragraph() {
    let content = r#"<w:txbxContent>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Heading</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr><w:del><w:r><w:delText>Item</w:delText></w:r></w:del></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:txbxContent>"#;
    let numbering = format!(
        r#"<w:numbering {W}><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
    );
    let expected = "## {--Heading--}\n\n- {--Item--}\n\n|{--Cell--}|\n|-|\n";
    for drawing in ["drawing", "pict", "object"] {
        let run = format!("<w:r><w:{drawing}>{content}</w:{drawing}></w:r>");
        for body in [
            p("", &format!("<w:del>{run}</w:del>")),
            format!("<w:del>{}</w:del>", p("", &run)),
            p(
                "",
                &format!(
                    r#"<w:del><w:hyperlink w:anchor="bookmark"><w:sdt><w:sdtContent>{run}</w:sdtContent></w:sdt></w:hyperlink></w:del>"#
                ),
            ),
        ] {
            assert_eq!(
                md(&docx(&body, &[("word/numbering.xml", &numbering)])),
                expected
            );
        }
    }
    let body = p(
        "",
        &format!(
            "{}<w:del><w:r><w:delText>Old</w:delText><w:drawing>{content}</w:drawing></w:r></w:del>",
            r("Keep ")
        ),
    );
    assert_eq!(
        md(&docx(&body, &[("word/numbering.xml", &numbering)])),
        format!("Keep {{--Old--}}\n\n{expected}")
    );
}

#[test]
fn deleted_row_with_inline_deletion_has_no_nested_delimiters() {
    let body = r#"<w:tbl>
<w:tr><w:tc><w:p><w:r><w:t>Header</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:del w:id="1"/></w:trPr><w:tc><w:p>
<w:r><w:t xml:space="preserve">Before </w:t></w:r>
<w:del w:id="2"><w:r><w:rPr><w:b/></w:rPr><w:delText>old</w:delText></w:r></w:del>
<w:r><w:t xml:space="preserve"> after</w:t></w:r>
</w:p></w:tc></w:tr></w:tbl>"#;
    assert_eq!(
        md(&docx(body, &[])),
        "|Header|\n|-|\n|{--Before **old** after--}|\n"
    );
}

#[test]
fn every_tracked_change_names_its_author_and_date_as_word_stores_them() {
    let t1 = "2026-09-29T14:05:00Z";
    let t2 = "2026-09-29T15:10:00Z";
    let body = [
        // One person's replacement: one note after the substitution.
        p(
            "",
            &format!(
                "{}{}",
                by("del", "Ana Lima", t1, &dr("fonts")),
                by("ins", "Ana Lima", t1, &r("styles"))
            ),
        ),
        // Two people's: both notes, old side first.
        p(
            "",
            &format!(
                "{}{}",
                by("del", "Ana Lima", t1, &dr("old")),
                by("ins", "Bo Chen", t2, &r("new"))
            ),
        ),
        // The same author at two times stays two changes.
        p(
            "",
            &format!(
                "{}{}",
                by("ins", "Ana Lima", t1, &r("a")),
                by("ins", "Ana Lima", t2, &r("b"))
            ),
        ),
        // Moves carry their own attribution.
        p("", &by("moveTo", "Bo Chen", t2, &r("moved here"))),
        // Missing date, missing author, neither, and a name that needs escaping.
        p(
            "",
            &format!(
                "{}{}{}{}",
                by("ins", "Ana Lima", "", &r("w")),
                by("del", "", t1, &dr("x")),
                by("ins", "", "", &r("y")),
                by("ins", "A <<} B", t1, &r("z")),
            ),
        ),
    ]
    .concat();
    assert_eq!(
        md(&docx(&body, &[])),
        "{~~fonts~>styles~~}{>>Ana Lima (2026-09-29T14:05:00Z)<<}\n\n\
             {~~old~>new~~}{>>Ana Lima (2026-09-29T14:05:00Z)<<}{>>Bo Chen (2026-09-29T15:10:00Z)<<}\n\n\
             {++a++}{>>Ana Lima (2026-09-29T14:05:00Z)<<}{++b++}{>>Ana Lima (2026-09-29T15:10:00Z)<<}\n\n\
             {++moved here++}{>>Bo Chen (2026-09-29T15:10:00Z)<<}\n\n\
             {~~x~>w~~}{>>(2026-09-29T14:05:00Z)<<}{>>Ana Lima<<}{++y++}{++z++}{>>A <<\\} B (2026-09-29T14:05:00Z)<<}\n"
    );
}

#[test]
fn paragraph_breaks_rows_and_cells_carry_their_own_attribution() {
    let t1 = "2026-09-29T14:05:00Z";
    let t2 = "2026-09-29T15:10:00Z";
    let mark = |author: &str, date: &str| {
        format!(
            r#"<w:pPr><w:rPr><w:del w:id="8" w:author="{author}" w:date="{date}"/></w:rPr></w:pPr>"#
        )
    };
    let body = [
        // Text deleted by Ana, the break after it by Bo: two changes.
        format!("<w:p>{}{}</w:p>", mark("Bo Chen", t2), by("del", "Ana Lima", t1, &dr("A"))),
        p("", &r("Next")),
        // Text and break both deleted by Ana at t1: one change.
        format!("<w:p>{}{}</w:p>", mark("Ana Lima", t1), by("del", "Ana Lima", t1, &dr("B"))),
        p("", &by("del", "Ana Lima", t1, &dr("C"))),
        format!(
            r#"<w:tbl><w:tr><w:trPr><w:ins w:id="9" w:author="Bo Chen" w:date="{t2}"/></w:trPr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellDel w:id="10" w:author="Ana Lima" w:date="{t1}"/></w:tcPr>{}</w:tc></w:tr></w:tbl>"#,
            p("", &r("row")),
            p("", &r("cell")),
        ),
    ]
    .concat();
    assert_eq!(
        md(&docx(&body, &[])),
        "{--A--}{>>Ana Lima (2026-09-29T14:05:00Z)<<}{--\n\n--}{>>Bo Chen (2026-09-29T15:10:00Z)<<}Next\n\n\
             {--B\n\nC--}{>>Ana Lima (2026-09-29T14:05:00Z)<<}\n\n\
             |{++row++}{>>Bo Chen (2026-09-29T15:10:00Z)<<}|{++{--cell--}{>>Ana Lima (2026-09-29T14:05:00Z)<<}++}{>>Bo Chen (2026-09-29T15:10:00Z)<<}|\n|-|-|\n"
    );
}

#[test]
fn accept_and_reject_resolve_paragraphs_breaks_and_moves() {
    let body = [
        pm("", &["del"], &del(&r("Gone"))),
        p("", &r("Next")),
        p("", &format!("{}{}", ins(&r("New ")), r("text"))),
        p("", &by("moveFrom", "Bo", "", &r("Moved away"))),
        p("", &by("moveTo", "Bo", "", &r("Moved here"))),
        pm("", &["ins"], &r("One")),
        p("", &r("two")),
    ]
    .concat();
    let bytes = docx(&body, &[]);
    assert_eq!(
        resolved(&bytes, Revisions::Accept),
        "Next\n\nNew text\n\nMoved here\n\nOne\n\ntwo\n"
    );
    assert_eq!(
        resolved(&bytes, Revisions::Reject),
        "Gone\n\nNext\n\ntext\n\nMoved away\n\nOnetwo\n"
    );
}

#[test]
fn a_break_that_goes_away_joins_across_empty_markers() {
    // Word writes bookmark and range ends between paragraphs; they hold no
    // content, so the paragraphs still join.
    for marker in [
        end("1"),
        r#"<w:bookmarkEnd w:id="0"/>"#.to_string(),
        r#"<w:moveToRangeEnd w:id="3"/>"#.to_string(),
    ] {
        let body = format!("{}{marker}{}", pm("", &["del"], &r("A")), p("", &r("B")));
        let bytes = docx(&body, &[]);
        assert_eq!(resolved(&bytes, Revisions::Accept), "AB\n", "{marker}");
        assert_eq!(resolved(&bytes, Revisions::Reject), "A\n\nB\n", "{marker}");
    }
    // A table between them is content: the break before it stays.
    let body = format!(
        "{}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>{}",
        pm("", &["del"], &r("A")),
        p("", &r("cell")),
        p("", &r("B"))
    );
    assert_eq!(
        resolved(&docx(&body, &[]), Revisions::Accept),
        "A\n\n|cell|\n|-|\n\nB\n"
    );
}

#[test]
fn a_joined_paragraph_takes_the_later_paragraphs_properties() {
    // Word keeps paragraph properties on the mark that ends the paragraph,
    // so deleting a break gives the joined text the second one's format.
    let heading =
        r#"<w:p><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>"#;
    let body = format!("{}{heading}", pm("", &["del"], &r("Intro ")));
    let bytes = docx(&body, &[]);
    assert_eq!(resolved(&bytes, Revisions::Accept), "# Intro Title\n");
    assert_eq!(resolved(&bytes, Revisions::Reject), "Intro\n\n# Title\n");
}

#[test]
fn accept_and_reject_resolve_rows_cells_and_nested_tables() {
    let body = format!(
        r#"<w:tbl><w:tr><w:tc>{}{}{}</w:tc><w:tc>{}</w:tc></w:tr><w:tr><w:trPr><w:ins/></w:trPr><w:tc><w:tbl><w:tr><w:trPr><w:del/></w:trPr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellDel/></w:tcPr>{}</w:tc></w:tr></w:tbl></w:tc><w:tc>{}</w:tc></w:tr><w:tr><w:tc>{}</w:tc><w:tc><w:tcPr><w:cellDel/></w:tcPr>{}</w:tc><w:tc><w:tcPr><w:cellIns/></w:tcPr>{}</w:tc></w:tr></w:tbl>"#,
        pm("", &["del"], &r("a")),
        pm("", &["ins"], &r("b")),
        pm("", &["del"], &r("c")),
        ins(&p("", &r("block ins"))),
        p("", &r("n1")),
        p("", &r("n2")),
        del(&p("", &r("wrapped"))),
        p("", &r("x")),
        p("", &r("gone")),
        p("", &r("added")),
    );
    let bytes = docx(&body, &[]);
    assert_eq!(
        resolved(&bytes, Revisions::Accept),
        "|ab<br>c|block ins|\n|-|-|\n|x|added|\n"
    );
    assert_eq!(
        resolved(&bytes, Revisions::Reject),
        "|a<br>bc||\n|-|-|\n|x|gone|\n"
    );
}

#[test]
fn accept_and_reject_resolve_text_boxes_and_footnotes() {
    let body = [
        p("", &del(&text_box(&p("", &r("Deleted box"))))),
        p(
            "",
            &format!(
                "{}{}{}",
                r("Keep"),
                ins(&text_box(&p("", &r("Added box")))),
                r#"<w:r><w:footnoteReference w:id="2"/></w:r>"#
            ),
        ),
    ]
    .concat();
    let footnotes = format!(
        r#"<w:footnotes {W}><w:footnote w:id="2">{}{}</w:footnote></w:footnotes>"#,
        pm(
            "",
            &["del"],
            &format!("{}{}", r("Source"), ins(&r(" updated")))
        ),
        p("", &r("page 4")),
    );
    let bytes = docx(&body, &[("word/footnotes.xml", &footnotes)]);
    assert_eq!(
        resolved(&bytes, Revisions::Accept),
        "Keep[^1]\n\nAdded box\n\n[^1]: Source updatedpage 4\n"
    );
    assert_eq!(
        resolved(&bytes, Revisions::Reject),
        "Deleted box\n\nKeep[^1]\n\n[^1]: Source page 4\n"
    );
}

#[test]
fn reject_restores_formatting_that_a_tracked_change_replaced() {
    let run = r#"<w:r><w:rPr><w:b/><w:rPrChange w:id="1" w:author="Ana"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr><w:t>styled</w:t></w:r>"#;
    let bytes = docx(&p("", run), &[]);
    // A formatting change alone is not shown as markup.
    assert_eq!(md(&bytes), "**styled**\n");
    assert_eq!(resolved(&bytes, Revisions::Accept), "**styled**\n");
    assert_eq!(resolved(&bytes, Revisions::Reject), "_styled_\n");
    // LibreOffice records a change from plain text with no earlier `w:rPr`
    // inside: rejecting it leaves the text plain.
    let run = r#"<w:r><w:rPr><w:b/><w:rPrChange w:id="1" w:author="Ana"></w:rPrChange></w:rPr><w:t>styled</w:t></w:r>"#;
    let bytes = docx(&p("", run), &[]);
    assert_eq!(resolved(&bytes, Revisions::Accept), "**styled**\n");
    assert_eq!(resolved(&bytes, Revisions::Reject), "styled\n");
}
