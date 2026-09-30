//! Ten documents heavy on tracked changes and comments, each checked against
//! LibreOffice Writer's own Accept All and Reject All.
//!
//! `fixtures/critic/synthesize.py` writes the `.fodt` files in
//! `fixtures/critic/synthetic` (see its header for the commands). Writer saves
//! each as Word (`NAME.docx`), and `libreoffice-oracle.py --docx` saves Writer's
//! Accept All and Reject All results as `NAME.accepted.docx` and
//! `NAME.rejected.docx`, with their plain text in `.txt`. `NAME.md` is the
//! CriticMarkup anymd writes for `NAME.docx`.
//!
//! For every document:
//! - `revisions: accept` gives the same Markdown as Writer's accepted file,
//!   and `revisions: reject` as its rejected file;
//! - accepting (or rejecting) the CriticMarkup of the default output gives
//!   that same Markdown too, so the markup says exactly what changed.

use std::path::{Path, PathBuf};

use anymd_formats::{convert, Format, Options, Revisions};

const DOCUMENTS: [&str; 10] = [
    "all",
    "comments",
    "deletions",
    "additions",
    "comments-deletions",
    "comments-additions",
    "substitutions",
    "breaks",
    "tables",
    "structure",
];

fn path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/critic/synthetic")
        .join(name)
}

fn markdown(name: &str, revisions: Revisions) -> String {
    let bytes = std::fs::read(path(name)).unwrap();
    let options = Options {
        revisions,
        ..Options::default()
    };
    convert(Format::Docx, &bytes, &options)
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

/// The Markdown after accepting (or rejecting) every change in its
/// CriticMarkup: comments and highlights drop their markup either way.
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

/// Paragraphs as Writer's plain text has them: one per line, no Markdown
/// syntax, no blank lines. Space that Markdown does not show (padding in a
/// table cell, a run of spaces) is normalised; a missing space is not.
fn lines(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let line = line
                .trim_start_matches('#')
                .replace("**", "")
                .replace('_', "");
            let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
            line.split('|').map(str::trim).collect::<Vec<_>>().join("|")
        })
        .collect()
}

#[test]
fn every_document_is_heavy_on_what_its_name_says() {
    for name in DOCUMENTS {
        let markup = markdown(&format!("{name}.docx"), Revisions::Markup);
        let count = |delimiter: &str| markup.matches(delimiter).count();
        let (inserted, deleted, substituted, commented) =
            (count("{++"), count("{--"), count("{~~"), count("<<}"));
        let changes = inserted + deleted + substituted;
        assert!(changes + commented >= 10, "{name}: {markup}");
        match name {
            "comments" => assert!(commented >= 10 && changes == 0, "{name}: {markup}"),
            "deletions" => assert!(deleted >= 10 && inserted == 0, "{name}: {markup}"),
            "additions" => assert!(inserted >= 10 && deleted == 0, "{name}: {markup}"),
            "substitutions" => assert!(substituted >= 5, "{name}: {markup}"),
            _ => {}
        }
    }
}

#[test]
fn the_markup_matches_the_golden_markdown() {
    for name in DOCUMENTS {
        let golden = std::fs::read_to_string(path(&format!("{name}.md"))).unwrap();
        assert_eq!(
            markdown(&format!("{name}.docx"), Revisions::Markup),
            golden,
            "{name}"
        );
    }
}

#[test]
fn revisions_accept_and_reject_match_writers_accept_all_and_reject_all() {
    for name in DOCUMENTS {
        for (revisions, result) in [
            (Revisions::Accept, "accepted"),
            (Revisions::Reject, "rejected"),
        ] {
            assert_eq!(
                markdown(&format!("{name}.docx"), revisions),
                markdown(&format!("{name}.{result}.docx"), revisions),
                "{name} {result}"
            );
        }
    }
}

#[test]
fn resolving_the_markup_matches_writers_accept_all_and_reject_all() {
    for name in DOCUMENTS {
        let markup = markdown(&format!("{name}.docx"), Revisions::Markup);
        for (accept, result, revisions) in [
            (true, "accepted", Revisions::Accept),
            (false, "rejected", Revisions::Reject),
        ] {
            let ours = lines(&resolve(&markup, accept));
            let writer = lines(&markdown(&format!("{name}.{result}.docx"), revisions));
            let differ = (0..ours.len().max(writer.len())).find(|&i| ours.get(i) != writer.get(i));
            assert!(
                differ.is_none(),
                "{name} {result}, line {differ:?}:\n  markup resolved: {:?}\n  Writer:          {:?}",
                differ.and_then(|i| ours.get(i)),
                differ.and_then(|i| writer.get(i)),
            );
        }
    }
}

#[test]
fn writers_plain_text_is_in_the_accepted_and_rejected_markdown() {
    // Writer's plain text leaves tables out, so each of its lines is looked
    // for in order in anymd's text rather than compared line for line.
    for name in DOCUMENTS {
        for (result, revisions) in [
            ("accepted", Revisions::Accept),
            ("rejected", Revisions::Reject),
        ] {
            let writer = std::fs::read_to_string(path(&format!("{name}.{result}.txt"))).unwrap();
            let ours = lines(&markdown(&format!("{name}.docx"), revisions)).join("\n");
            let mut from = 0;
            for line in writer.lines().filter(|line| !line.trim().is_empty()) {
                let found = ours[from..].find(line.trim());
                assert!(
                    found.is_some(),
                    "{name} {result}: {line:?} not in order in\n{ours}"
                );
                from += found.unwrap() + line.trim().len();
            }
        }
    }
}
