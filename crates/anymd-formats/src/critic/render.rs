//! Renders normalized spans, preserving Markdown emphasis and link boundaries.

use super::spans::{Leaf, Node};
use super::{note, Mark};
use crate::ooxml::Inline;

pub(super) fn inline(nodes: &[Node], plain: bool) -> Inline {
    let mut pieces = Vec::new();
    render(nodes, &mut pieces, plain, &mut 0);
    wrap_markers(&mut pieces);
    let mut inline = Inline::default();
    for piece in pieces {
        inline.push(&piece.text, piece.bold, piece.italic, piece.link.as_deref());
    }
    inline
}

/// One piece of rendered inline text. Markers are CriticMarkup delimiters and
/// notes, which take a link only from the text around them.
struct Piece {
    text: String,
    bold: bool,
    italic: bool,
    link: Option<String>,
    /// For a marker, the change or comment it belongs to: all of one change's
    /// markers join a link, or none do, so brackets never cross.
    marker: Option<usize>,
}

fn render(nodes: &[Node], out: &mut Vec<Piece>, plain: bool, ids: &mut usize) {
    let mut index = 0;
    while index < nodes.len() {
        match (&nodes[index], nodes.get(index + 1)) {
            (
                Node::Span(Mark::Deletion, old_by, old),
                Some(Node::Span(Mark::Insertion, new_by, new)),
            )
            | (
                Node::Span(Mark::Insertion, new_by, new),
                Some(Node::Span(Mark::Deletion, old_by, old)),
            ) => {
                *ids += 1;
                let id = *ids;
                marker(out, "{~~", id);
                render(old, out, plain, ids);
                marker(out, "~>", id);
                render(new, out, plain, ids);
                marker(out, "~~}", id);
                attribution(out, old_by.as_deref(), id);
                if new_by != old_by {
                    attribution(out, new_by.as_deref(), id);
                }
                index += 2;
                continue;
            }
            (Node::Span(mark, by, nodes), _) => {
                *ids += 1;
                let id = *ids;
                let (open, close) = mark.delimiters();
                marker(out, open, id);
                render(nodes, out, plain, ids);
                marker(out, close, id);
                attribution(out, by.as_deref(), id);
            }
            (
                Node::Leaf(Leaf::Text {
                    text,
                    bold,
                    italic,
                    link,
                }),
                _,
            ) => {
                out.push(Piece {
                    text: if plain { text.clone() } else { escape(text) },
                    bold: *bold,
                    italic: *italic,
                    link: link.clone(),
                    marker: None,
                });
            }
            (Node::Leaf(Leaf::Raw(markdown)), _) => out.push(Piece {
                text: if plain {
                    markdown.clone()
                } else {
                    defuse(markdown)
                },
                bold: false,
                italic: false,
                link: None,
                marker: None,
            }),
            (Node::Leaf(Leaf::Comment(inner)), _) => {
                *ids += 1;
                marker(out, &note(inner), *ids);
            }
        }
        index += 1;
    }
}

fn marker(out: &mut Vec<Piece>, text: &str, id: usize) {
    out.push(Piece {
        text: text.to_string(),
        bold: false,
        italic: false,
        link: None,
        marker: Some(id),
    });
}

/// Markers between two pieces of the same link or emphasis take it on, so a
/// change inside a link's text or a bold run keeps one link or one run around
/// it: `[the {++new ++}page](url)`, `**around {++this++} inside**`.
/// CriticMarkup is read before Markdown, so the link and the emphasis still
/// resolve. A change joins only if all its markers do; one that starts inside
/// the link and ends after it stays outside.
fn wrap_markers(pieces: &mut [Piece]) {
    type Format = (bool, bool, Option<String>);
    let format = |piece: &Piece| -> Format { (piece.bold, piece.italic, piece.link.clone()) };
    let mut joined: Vec<Option<Format>> = vec![None; pieces.len()];
    let mut start = 0;
    while start < pieces.len() {
        if pieces[start].marker.is_none() {
            start += 1;
            continue;
        }
        let end = (start..pieces.len())
            .find(|&at| pieces[at].marker.is_none())
            .unwrap_or(pieces.len());
        let before = start.checked_sub(1).map(|at| format(&pieces[at]));
        let after = pieces.get(end).map(format);
        if let Some(around) =
            before.filter(|f| *f != (false, false, None) && Some(f) == after.as_ref())
        {
            joined[start..end].fill(Some(around));
        }
        start = end;
    }
    let refused: std::collections::HashSet<usize> = pieces
        .iter()
        .zip(&joined)
        .filter(|(piece, around)| around.is_none() && piece.marker.is_some())
        .filter_map(|(piece, _)| piece.marker)
        .collect();
    for (piece, around) in pieces.iter_mut().zip(joined) {
        if let Some((bold, italic, link)) =
            around.filter(|_| piece.marker.is_some_and(|id| !refused.contains(&id)))
        {
            piece.bold = bold;
            piece.italic = italic;
            piece.link = link;
        }
    }
}

fn attribution(out: &mut Vec<Piece>, by: Option<&str>, id: usize) {
    if let Some(by) = by {
        marker(out, &note(by), id);
    }
}

/// Breaks every CriticMarkup delimiter in document text with a Markdown
/// backslash escape, so it renders unchanged but cannot open or close a span.
pub(crate) fn escape(text: &str) -> String {
    const DELIMITERS: [(&str, &str); 11] = [
        ("{++", "{\\++"),
        ("{--", "{\\--"),
        ("{~~", "{\\~~"),
        ("{>>", "{\\>>"),
        ("{==", "{\\=="),
        ("++}", "++\\}"),
        ("--}", "--\\}"),
        ("~~}", "~~\\}"),
        ("<<}", "<<\\}"),
        ("==}", "==\\}"),
        ("~>", "~\\>"),
    ];
    if !text.contains(['{', '}', '~']) {
        return text.to_string();
    }
    DELIMITERS
        .iter()
        .fold(text.to_string(), |text, (from, to)| text.replace(from, to))
}

/// Breaks every CriticMarkup delimiter in Markdown the converter built
/// (equations, image descriptions) with a space. A backslash would change the
/// LaTeX; a space does not, since LaTeX ignores spaces in math, and an image
/// description reads the same.
pub(super) fn defuse(markdown: &str) -> String {
    const DELIMITERS: [(&str, &str); 11] = [
        ("{++", "{ ++"),
        ("{--", "{ --"),
        ("{~~", "{ ~~"),
        ("{>>", "{ >>"),
        ("{==", "{ =="),
        ("++}", "++ }"),
        ("--}", "-- }"),
        ("~~}", "~~ }"),
        ("<<}", "<< }"),
        ("==}", "== }"),
        ("~>", "~ >"),
    ];
    DELIMITERS
        .iter()
        .fold(markdown.to_string(), |text, (from, to)| {
            text.replace(from, to)
        })
}
