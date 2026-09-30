//! Word tracked changes and comments as CriticMarkup, on a document written by an
//! office suite rather than by these tests.
//!
//! `fixtures/critic/tracked-changes.docx` is `tracked-changes.fodt` saved as Word
//! by LibreOffice Writer 24.2:
//!
//! ```sh
//! soffice --headless --convert-to "docx:MS Word 2007 XML" tracked-changes.fodt
//! ```
//!
//! `tracked-changes.accepted.txt` and `tracked-changes.rejected.txt` are Writer's
//! own "Accept All" and "Reject All" results for that file, written by
//! `libreoffice-oracle.py`. The Markdown must give the same text when its
//! CriticMarkup is accepted or rejected.

use anymd_formats::{convert, Format, Options};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/critic")
            .join(name),
    )
    .unwrap()
}

fn markdown() -> String {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/critic/tracked-changes.docx"),
    )
    .unwrap();
    convert(Format::Docx, &bytes, &Options::default())
        .unwrap()
        .sections
        .remove(0)
        .markdown
}

/// Replaces each `open…close` span with `keep(inner)`, matching the closer
/// lazily, as the CriticMarkup toolkit's regular expressions do.
fn spans(text: &str, open: &str, close: &str, keep: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        let inner = start + open.len();
        let Some(length) = rest[inner..].find(close) else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str(&keep(&rest[inner..inner + length]));
        rest = &rest[inner + length + close.len()..];
    }
    out.push_str(rest);
    out
}

/// The text after accepting (or rejecting) every change: comments and
/// highlights drop their markup either way.
fn resolve(markdown: &str, accept: bool) -> String {
    let pick = |kept: bool, text: &str| {
        if kept {
            text.to_string()
        } else {
            String::new()
        }
    };
    let text = spans(markdown, "{>>", "<<}", |_| String::new());
    let text = spans(&text, "{==", "==}", str::to_string);
    let text = spans(&text, "{~~", "~~}", |inner| {
        let (old, new) = inner.split_once("~>").unwrap();
        if accept { new } else { old }.to_string()
    });
    let text = spans(&text, "{++", "++}", |inner| pick(accept, inner));
    spans(&text, "{--", "--}", |inner| pick(!accept, inner))
}

/// One line per paragraph without Markdown syntax, as Writer returns text.
fn plain(markdown: &str) -> String {
    let lines: Vec<String> = markdown
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_start_matches("# ").replace("**", ""))
        .collect();
    lines.join("\n") + "\n"
}

#[test]
fn office_written_tracked_changes_match_the_golden_markdown() {
    assert_eq!(markdown(), fixture("tracked-changes.md"));
}

#[test]
fn accepting_the_markup_gives_the_office_accept_all_text() {
    assert_eq!(
        plain(&resolve(&markdown(), true)),
        fixture("tracked-changes.accepted.txt")
    );
}

#[test]
fn rejecting_the_markup_gives_the_office_reject_all_text() {
    assert_eq!(
        plain(&resolve(&markdown(), false)),
        fixture("tracked-changes.rejected.txt")
    );
}

#[test]
fn every_comment_names_its_author_and_word_date() {
    let markdown = markdown();
    assert!(markdown.contains("{>>Bill Winter (2024-04-08T10:32:00Z): true<<}"));
    assert!(markdown.contains("{>>Ana Lima (2026-09-30T08:15:00Z): This is a comment<<}"));
}

#[test]
fn every_change_names_its_author_and_date() {
    let markdown = markdown();
    for change in [
        "{-- to people that--}{>>Bo Chen (2026-09-29T15:12:00Z)<<}",
        "{~~fonts~>font-styles~~}{>>Ana Lima (2026-09-29T14:05:00Z)<<}",
        // LibreOffice splits this insertion in two; one author, one note.
        "{++**any** ++}{>>Bo Chen (2026-09-29T15:10:00Z)<<}",
        "{--This whole paragraph was cut.\n\n--}{>>Ana Lima (2026-09-30T09:00:00Z)<<}",
        "{++\n\n++}{>>Bo Chen (2026-09-30T09:30:00Z)<<}",
    ] {
        assert!(markdown.contains(change), "{change} not in {markdown}");
    }
}
