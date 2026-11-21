use std::io::Write;

use super::{convert, ConvertError, Options, Revisions};

// Keep the docx::tests prefix used by the focused CI test command while
// placing each capability's tests beside the corresponding implementation.
#[path = "review/comments_tests.rs"]
mod comments;
#[path = "inline/tests.rs"]
mod inline;
#[path = "notes/tests.rs"]
mod notes;
#[path = "numbering/tests.rs"]
mod numbering;
#[path = "review/revisions_tests.rs"]
mod revisions;
#[path = "styles/tests.rs"]
mod styles;
#[path = "tables/tests.rs"]
mod tables;

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

pub(crate) fn zip(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in parts {
        writer.start_file(*name, options).unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn docx(body: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>{body}</w:body></w:document>"#
    );
    let mut parts: Vec<(&str, &str)> = vec![
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/document.xml", &document),
    ];
    parts.extend_from_slice(extra);
    zip(&parts)
}

fn md(bytes: &[u8]) -> String {
    convert(bytes, &Options::default()).unwrap().sections[0]
        .markdown
        .clone()
}

fn p(style: &str, runs: &str) -> String {
    let ppr = if style.is_empty() {
        String::new()
    } else {
        format!(r#"<w:pPr><w:pStyle w:val="{style}"/></w:pPr>"#)
    };
    format!("<w:p>{ppr}{runs}</w:p>")
}

fn r(text: &str) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{text}</w:t></w:r>"#)
}

// Tracked-change building blocks, written the way Word writes them.
fn ins(content: &str) -> String {
    format!(r#"<w:ins w:id="90" w:author="Ana" w:date="2026-01-02T03:04:00Z">{content}</w:ins>"#)
}

fn del(content: &str) -> String {
    format!(r#"<w:del w:id="91" w:author="Ana" w:date="2026-01-02T03:04:00Z">{content}</w:del>"#)
}

fn dr(text: &str) -> String {
    format!(r#"<w:r><w:delText xml:space="preserve">{text}</w:delText></w:r>"#)
}

/// A paragraph whose mark (the break after it) carries `mark` (`ins`, `del`,
/// `moveFrom`, `moveTo`, or several), with an optional style.
fn pm(style: &str, marks: &[&str], runs: &str) -> String {
    let style = if style.is_empty() {
        String::new()
    } else {
        format!(r#"<w:pStyle w:val="{style}"/>"#)
    };
    let marks: String = marks
        .iter()
        .map(|m| format!(r#"<w:{m} w:id="92" w:author="Ana" w:date="2026-01-02T03:04:00Z"/>"#))
        .collect();
    format!("<w:p><w:pPr>{style}<w:rPr>{marks}</w:rPr></w:pPr>{runs}</w:p>")
}

fn text_box(inner: &str) -> String {
    format!(
        r#"<w:r><w:drawing><wp:anchor xmlns:wp="wp"><wp:docPr id="7" name="Text Box 7"/><a:graphic xmlns:a="a"><a:graphicData><wps:wsp xmlns:wps="wps"><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#
    )
}

/// `word/comments.xml` with one `w:comment` per entry: (id, author, date,
/// paragraphs). An empty author or date leaves the attribute out.
fn comments(entries: &[(&str, &str, &str, &str)]) -> String {
    let body: String = entries
        .iter()
        .map(|(id, author, date, paragraphs)| {
            let author = if author.is_empty() {
                String::new()
            } else {
                format!(r#" w:author="{author}""#)
            };
            let date = if date.is_empty() {
                String::new()
            } else {
                format!(r#" w:date="{date}""#)
            };
            format!(
                r#"<w:comment w:id="{id}"{author}{date} w:initials="X">{paragraphs}</w:comment>"#
            )
        })
        .collect();
    format!(r#"<w:comments {W}>{body}</w:comments>"#)
}

/// The reference run Word writes right after a comment's range end.
fn reference(id: &str) -> String {
    format!(
        r#"<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="{id}"/></w:r>"#
    )
}

/// A revision element with an explicit author and date (either may be empty
/// to leave the attribute out).
fn by(tag: &str, author: &str, date: &str, content: &str) -> String {
    let author = if author.is_empty() {
        String::new()
    } else {
        format!(r#" w:author="{author}""#)
    };
    let date = if date.is_empty() {
        String::new()
    } else {
        format!(r#" w:date="{date}""#)
    };
    format!(r#"<w:{tag} w:id="7"{author}{date}>{content}</w:{tag}>"#)
}

fn end(id: &str) -> String {
    format!(r#"<w:commentRangeEnd w:id="{id}"/>"#)
}

fn resolved(bytes: &[u8], revisions: Revisions) -> String {
    let options = Options {
        revisions,
        ..Options::default()
    };
    convert(bytes, &options).unwrap().sections[0]
        .markdown
        .clone()
}

#[test]
fn malformed_input_is_invalid_not_panic() {
    assert!(matches!(
        convert(b"not a zip", &Options::default()),
        Err(ConvertError::Invalid(_))
    ));
    let no_doc = zip(&[("hello.txt", "hi")]);
    assert!(matches!(
        convert(&no_doc, &Options::default()),
        Err(ConvertError::Invalid(_))
    ));
    let broken = zip(&[("word/document.xml", "<w:document><w:body><w:p></w:body>")]);
    assert!(convert(&broken, &Options::default()).is_err());
    let truncated = docx(&p("", &r("ok")), &[]);
    assert!(convert(&truncated[..truncated.len() / 2], &Options::default()).is_err());
}
