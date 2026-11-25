use std::io::Cursor;

use crate::ConvertError;

use super::package::resolve_target;
use super::{parse_xml, Inline, ListIndent, Package};

fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        archive
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

#[test]
fn package_lookup_preserves_exact_names_and_ascii_fallback() {
    let bytes = zip_entries(&[
        ("word/document.xml", b"lower".as_slice()),
        ("WORD/DOCUMENT.XML", b"upper".as_slice()),
        ("word/Styles.xml", b"styles".as_slice()),
        ("word/\u{c4}.xml", b"unicode".as_slice()),
    ]);
    let mut package = Package::open(&bytes, "DOCX").unwrap();
    for (name, expected) in [
        ("word/document.xml", b"lower".as_slice()),
        ("/WORD/DOCUMENT.XML", b"upper".as_slice()),
        ("/WORD/styles.XML", b"styles".as_slice()),
        ("WORD/\u{c4}.XML", b"unicode".as_slice()),
    ] {
        assert!(package.has(name));
        assert_eq!(package.read(name).unwrap().as_deref(), Some(expected));
    }
    for missing in ["word/missing.xml", "word/\u{e4}.xml"] {
        assert!(!package.has(missing));
        assert_eq!(package.read(missing).unwrap(), None);
    }
    assert!(matches!(
        Package::open(b"not a zip", "DOCX"),
        Err(ConvertError::Invalid(_))
    ));
}

#[test]
fn decoded_exact_name_does_not_fall_through_to_a_case_variant() {
    let mut bytes = zip_entries(&[("WORD/\u{c4}.xml", b"fallback"), ("word/X.xml", b"exact")]);
    // CP437 encodes Ä as 0x8e. Replace the one-byte ASCII placeholder in
    // both ZIP name fields without changing header lengths or file data.
    let placeholder = b"word/X.xml";
    for index in 0..=bytes.len() - placeholder.len() {
        if &bytes[index..index + placeholder.len()] == placeholder {
            bytes[index + 5] = 0x8e;
        }
    }
    let mut package = Package::open(&bytes, "DOCX").unwrap();
    assert!(package.has("word/\u{c4}.xml"));
    // zip 9.1 resolves decoded CP437 names. Read the exact entry, not the
    // differently cased fallback, when both names are present.
    assert_eq!(
        package.read("word/\u{c4}.xml").unwrap().as_deref(),
        Some(b"exact".as_slice())
    );
}

#[test]
fn resolves_relative_targets() {
    assert_eq!(
        resolve_target("ppt/slides", "../notesSlides/n1.xml"),
        "ppt/notesSlides/n1.xml"
    );
    assert_eq!(resolve_target("word", "media/a.png"), "word/media/a.png");
    assert_eq!(resolve_target("word", "/word/x.xml"), "word/x.xml");
    assert_eq!(resolve_target("", "word/document.xml"), "word/document.xml");
}

#[test]
fn inline_merges_and_places_markers() {
    let mut inline = Inline::default();
    inline.push("Hello ", true, false, None);
    inline.push("world", true, false, None);
    inline.push(" and ", false, false, None);
    inline.push("site", false, true, Some("https://x.y"));
    inline.push("   ", true, false, None);
    assert_eq!(
        inline.render(true),
        "**Hello world** and [_site_](https://x.y)"
    );
    assert_eq!(inline.render(false), "Hello world and [site](https://x.y)");
}

#[test]
fn xml_entities_and_depth() {
    let root = parse_xml(b"<a x=\"1&amp;2\">x &lt; y &#x41;<![CDATA[<z>]]></a>").unwrap();
    assert_eq!(root.attr("x"), Some("1&2"));
    assert_eq!(root.text(), "x < y A<z>");
    let deep = "<a>".repeat(2000);
    assert!(parse_xml(deep.as_bytes()).is_err());
    assert!(parse_xml(b"").is_err());
}

#[test]
fn list_indent_nests_under_content() {
    let mut list = ListIndent::default();
    assert_eq!(list.item(0, "10.", "a"), "10. a");
    assert_eq!(list.item(1, "-", "b"), "    - b");
    assert_eq!(list.item(0, "11.", "c\nd"), "11. c\n    d");
}
