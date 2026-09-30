//! Twenty documents heavy on tracked changes and comments, each checked
//! against LibreOffice Writer's own Accept All and Reject All.
//!
//! `fixtures/critic/synthesize.py` writes ten seeded `.fodt` files in
//! `fixtures/critic/synthetic`, and `challenging.py` ten more by hand
//! (`hard-*`), each around one hard case (see their headers for the commands). Writer saves
//! each as Word (`NAME.docx`), and `libreoffice-oracle.py --docx` saves Writer's
//! Accept All and Reject All results as `NAME.accepted.docx` and
//! `NAME.rejected.docx`, with their plain text in `.txt`.
//!
//! anymd's own output for `NAME.docx` is kept as the source of truth for
//! regressions: `NAME.critic.md` (the default, with CriticMarkup),
//! `NAME.accept.md` and `NAME.reject.md` (`revisions: accept` and `reject`,
//! without it). After an intended change, rewrite them with
//! `ANYMD_BLESS=1 cargo test -p anymd-formats --test critic_synthetic` and
//! review the diff. Where Writer's result is wrong, the check against Writer
//! uses anymd's checked file instead, and `WRITER_WRONG` says why.
//!
//! For every document:
//! - `revisions: accept` gives the same Markdown as Writer's accepted file,
//!   and `revisions: reject` as its rejected file;
//! - accepting (or rejecting) the CriticMarkup of the default output gives
//!   that same Markdown too, so the markup says exactly what changed.

use std::path::{Path, PathBuf};

use anymd_formats::{convert, Format, Options, Revisions};

const DOCUMENTS: [&str; 20] = [
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
    // Hand-built around one hard case each (challenging.py).
    "hard-delimiters",
    "hard-unicode",
    "hard-links",
    "hard-notes",
    "hard-lists",
    "hard-comments",
    "hard-adjacent",
    "hard-headings",
    "hard-formatting",
    "hard-tables",
];

/// Results where Writer's Accept All or Reject All disagrees with its own
/// document, and why. anymd's checked output (`NAME.accept.md` or
/// `NAME.reject.md`) stands in for it.
const WRITER_WRONG: [(&str, &str, &str); 1] = [(
    "hard-links",
    "rejected",
    "rejecting an insertion that is a whole link leaves the full stop after it \
     linked, `[.](https://example.com/b)`, though the stop is outside the link \
     in both the .fodt and the .docx",
)];

/// The Markdown `revisions` should give for `result` (`accepted` or
/// `rejected`): Writer's own result, unless Writer is known to be wrong.
fn expected(name: &str, result: &str, revisions: Revisions) -> String {
    if WRITER_WRONG
        .iter()
        .any(|(n, r, _why)| *n == name && *r == result)
    {
        let golden = if result == "accepted" { "accept" } else { "reject" };
        return std::fs::read_to_string(path(&format!("{name}.{golden}.md"))).unwrap();
    }
    markdown(&format!("{name}.{result}.docx"), revisions)
}

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
    let text = spans(&text, "{--", "--}", |inner| pick(!accept, inner));
    // Delimiters in the document's own text are escaped with a backslash,
    // which Markdown drops when it renders them.
    [("{\\", "{"), ("\\}", "}"), ("~\\>", "~>")]
        .iter()
        .fold(text, |text, (from, to)| text.replace(from, to))
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

/// Footnote labels renumbered in order of first appearance, as a Markdown
/// renderer numbers them: the markup keeps the numbers of the marked-up
/// document, and a resolved note leaves a gap.
fn renumber(markdown: &str) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut out = String::new();
    let mut rest = markdown;
    while let Some(at) = rest.find("[^") {
        let Some(len) = rest[at + 2..].find(']') else {
            break;
        };
        let label = rest[at + 2..at + 2 + len].to_string();
        let number = match seen.iter().position(|l| *l == label) {
            Some(i) => i + 1,
            None => {
                seen.push(label);
                seen.len()
            }
        };
        out.push_str(&rest[..at]);
        out.push_str(&format!("[^{number}]"));
        rest = &rest[at + 3 + len..];
    }
    out.push_str(rest);
    out
}

/// A line without note citations: `[^N]` in Markdown, `N` or a roman numeral
/// (endnotes) right after a word in Writer's plain text.
fn without_citations(line: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '[' && chars.get(at + 1) == Some(&'^') {
            if let Some(len) = chars[at..].iter().position(|c| *c == ']') {
                at += len + 1;
                continue;
            }
        }
        let citation = |c: &char| c.is_ascii_digit() || "ivx".contains(*c);
        if at > 0 && !chars[at - 1].is_whitespace() && citation(&chars[at]) {
            let end = (at..chars.len()).find(|&i| !citation(&chars[i])).unwrap_or(chars.len());
            let alone = chars.get(end).is_none_or(|c| c.is_whitespace() || c.is_ascii_punctuation());
            // After a word, or after a word and its punctuation (`control.1`).
            let word = |c: &char| c.is_alphabetic() && !citation(c);
            let after_word = word(&chars[at - 1])
                || (chars[at - 1].is_ascii_punctuation() && at > 1 && word(&chars[at - 2]));
            if alone && after_word && chars[at].is_ascii_digit() == chars[at..end].iter().all(char::is_ascii_digit) {
                at = end;
                continue;
            }
        }
        out.push(chars[at]);
        at += 1;
    }
    out
}

/// `[text](url)` as `text`, as Writer's plain text has it.
fn link_text(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let label = &rest[open + 1..];
        let Some((text, after)) = label.split_once("](") else {
            break;
        };
        let Some(close) = after.find(')') else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(text);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

#[test]
fn every_document_is_heavy_on_what_its_name_says() {
    for &name in &DOCUMENTS[..10] {
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
fn every_output_matches_its_source_of_truth() {
    let bless = std::env::var_os("ANYMD_BLESS").is_some();
    for name in DOCUMENTS {
        for (revisions, kind) in [
            (Revisions::Markup, "critic"),
            (Revisions::Accept, "accept"),
            (Revisions::Reject, "reject"),
        ] {
            let ours = markdown(&format!("{name}.docx"), revisions);
            let golden = path(&format!("{name}.{kind}.md"));
            if bless {
                std::fs::write(&golden, &ours).unwrap();
                continue;
            }
            let golden = std::fs::read_to_string(&golden)
                .unwrap_or_else(|_| panic!("{name}.{kind}.md is missing; run with ANYMD_BLESS=1"));
            assert_eq!(ours, golden, "{name}.{kind}.md");
        }
    }
    // Without CriticMarkup means without it: a delimiter appears only as often
    // as in Writer's own text (hard-delimiters has some as plain text).
    for name in DOCUMENTS {
        for (kind, result) in [("accept", "accepted"), ("reject", "rejected")] {
            let ours = lines(&std::fs::read_to_string(path(&format!("{name}.{kind}.md"))).unwrap()).join("\n");
            let writer = lines(&std::fs::read_to_string(path(&format!("{name}.{result}.txt"))).unwrap()).join("\n");
            for delimiter in ["{++", "{--", "{~~", "{==", "{>>", "<<}"] {
                assert_eq!(
                    ours.matches(delimiter).count(),
                    writer.matches(delimiter).count(),
                    "{name}.{kind}.md: {delimiter}"
                );
            }
        }
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
                expected(name, result, revisions),
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
            let ours = lines(&renumber(&resolve(&markup, accept)));
            let writer = lines(&renumber(&expected(name, result, revisions)));
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
            let ours = without_citations(&link_text(
                &lines(&markdown(&format!("{name}.docx"), revisions)).join("\n"),
            ));
            let mut from = 0;
            for line in lines(&writer).iter().map(|line| without_citations(line)) {
                let found = ours[from..].find(line.as_str());
                assert!(
                    found.is_some(),
                    "{name} {result}: {line:?} not in order in\n{ours}"
                );
                from += found.unwrap() + line.len();
            }
        }
    }
}
