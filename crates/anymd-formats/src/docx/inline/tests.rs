use super::{docx, md, p, W};

#[test]
fn footnotes_images_and_fields() {
    let body = [
        p(
            "",
            r#"<w:r><w:t>Claim</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r>"#,
        ),
        p(
            "",
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> HYPERLINK "https://f.example" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>field link</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
        ),
        p(
            "",
            r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="1" name="Picture 1" descr="A chart of sales"/><a:graphic xmlns:a="a"><a:graphicData><pic:pic xmlns:pic="p"><pic:blipFill><a:blip r:embed="rIdImg"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
        ),
        p("", r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="2" name="Picture 2"/></wp:inline></w:drawing></w:r>"#),
    ]
    .concat();
    let footnotes = format!(
        r#"<w:footnotes {W}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> Source: survey.</w:t></w:r></w:p></w:footnote></w:footnotes>"#
    );
    let bytes = docx(
        &body,
        &[
            ("word/footnotes.xml", &footnotes),
            (
                "word/_rels/document.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#,
            ),
        ],
    );
    assert_eq!(
        md(&bytes),
        "Claim[^1]\n\n[field link](https://f.example)\n\n![A chart of sales](image1.png)\n\n[^1]: Source: survey.\n"
    );
}

#[test]
fn image_names_keep_delimiter_characters_out_of_the_markup() {
    let image = r#"<w:r><w:drawing><wp:inline xmlns:wp="wp"><wp:docPr id="1" name="P" descr="x"/><a:graphic xmlns:a="a"><a:graphicData><pic:pic xmlns:pic="p"><pic:blipFill><a:blip r:embed="rIdImg"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#;
    let bytes = docx(
        &p("", &format!("<w:del>{image}</w:del>")),
        &[(
            "word/_rels/document.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/a--}~&gt;{b}&lt;.png"/></Relationships>"#,
        )],
    );
    // `{`, `}`, `<` and `>` may not appear in a URI; encoded, they cannot form a
    // delimiter, so the destination reaches the Markdown unchanged.
    assert_eq!(md(&bytes), "{--![x](a--%7D~%3E%7Bb%7D%3C.png)--}\n");
}
