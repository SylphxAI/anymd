//! Normalizes flat converter events into well-nested CriticMarkup spans.

use super::Mark;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Leaf {
    /// Document text: escaped when rendered.
    Text {
        text: String,
        bold: bool,
        italic: bool,
        link: Option<String>,
    },
    /// Markdown built by the converter (images, math, note references).
    Raw(String),
    /// The inside of a `{>>…<<}` comment, already escaped.
    Comment(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Token {
    Leaf(Leaf),
    Open(Mark, Option<String>),
    Close(Mark),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Node {
    Leaf(Leaf),
    Span(Mark, Option<String>, Vec<Node>),
}

impl Node {
    fn has_content(&self) -> bool {
        match self {
            Self::Leaf(Leaf::Comment(_)) => false,
            Self::Leaf(_) => true,
            Self::Span(_, _, children) => children.iter().any(Self::has_content),
        }
    }
}

/// A span still open while the tree is built.
type Open = (Mark, Option<String>, Vec<Node>);

/// Builds well-nested spans from flat tokens. A span of a kind that is already
/// open is merged into the open one, which keeps its attribution; a close that
/// would cross other spans closes them first and opens them again after it,
/// with the same attribution. Spans still open at the end close.
pub(super) fn tree(tokens: Vec<Token>) -> Vec<Node> {
    let mut root = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    let mut merged = [0usize; 3];
    for token in tokens {
        match token {
            Token::Leaf(leaf) => children(&mut root, &mut open).push(Node::Leaf(leaf)),
            Token::Open(mark, _) if open.iter().any(|(m, ..)| *m == mark) => {
                merged[mark.index()] += 1;
            }
            Token::Open(mark, by) => open.push((mark, by, Vec::new())),
            Token::Close(mark) if merged[mark.index()] > 0 => merged[mark.index()] -= 1,
            Token::Close(mark) => {
                let Some(at) = open.iter().position(|(m, ..)| *m == mark) else {
                    continue;
                };
                let reopen: Vec<(Mark, Option<String>)> = open[at + 1..]
                    .iter()
                    .map(|(m, by, _)| (*m, by.clone()))
                    .collect();
                close_from(&mut root, &mut open, at);
                open.extend(reopen.into_iter().map(|(m, by)| (m, by, Vec::new())));
            }
        }
    }
    close_from(&mut root, &mut open, 0);
    root
}

fn children<'a>(root: &'a mut Vec<Node>, open: &'a mut [Open]) -> &'a mut Vec<Node> {
    match open.last_mut() {
        Some((_, _, nodes)) => nodes,
        None => root,
    }
}

/// Closes the open spans from index `at` up, innermost first, into their parent.
fn close_from(root: &mut Vec<Node>, open: &mut Vec<Open>, at: usize) {
    let closed = open.split_off(at);
    let outermost = closed
        .into_iter()
        .rev()
        .fold(None, |inner, (mark, by, mut nodes)| {
            nodes.extend(inner);
            Some(Node::Span(mark, by, nodes))
        });
    children(root, open).extend(outermost);
}

/// Drops empty spans, joins neighbours of the same kind and attribution, and
/// moves comments out of highlights, at every level.
pub(super) fn tidy(nodes: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();
    for node in nodes {
        let Node::Span(mark, by, nodes) = node else {
            out.push(node);
            continue;
        };
        let mut comments = Vec::new();
        let nodes = if mark == Mark::Highlight {
            lift_comments(nodes, &mut comments)
        } else {
            nodes
        };
        if !nodes.iter().any(Node::has_content) {
            lift_comments(nodes, &mut out);
        } else if let Some(Node::Span(.., before)) = out
            .last_mut()
            .filter(|last| matches!(last, Node::Span(m, b, _) if *m == mark && *b == by))
        {
            before.extend(nodes);
        } else {
            out.push(Node::Span(mark, by, nodes));
        }
        out.extend(comments);
    }
    join_text(
        out.into_iter()
            .map(|node| match node {
                Node::Span(mark, by, nodes) => Node::Span(mark, by, tidy(nodes)),
                leaf => leaf,
            })
            .collect(),
    )
}

/// Joins neighbouring text of the same formatting. Word splits runs anywhere,
/// so a delimiter can arrive in two pieces (`a -` and `-} b`); escaping the
/// joined text sees it whole.
fn join_text(nodes: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for node in nodes {
        match (out.last_mut(), node) {
            (
                Some(Node::Leaf(Leaf::Text {
                    text: before,
                    bold: b,
                    italic: i,
                    link: l,
                })),
                Node::Leaf(Leaf::Text {
                    text,
                    bold,
                    italic,
                    link,
                }),
            ) if *b == bold && *i == italic && *l == link => before.push_str(&text),
            (_, node) => out.push(node),
        }
    }
    out
}

/// Moves the space before an insertion or deletion that ends the paragraph
/// into it: `in{++ nested++}`, not `in {++nested++}`. Both read the same once
/// accepted, but rejected the second leaves a trailing space, which shows
/// when more text follows on the same line (`in ; gone` in a flattened table).
pub(super) fn space_into_last_change(nodes: &mut [Node]) {
    let [.., Node::Leaf(Leaf::Text { text, .. }), Node::Span(Mark::Insertion | Mark::Deletion, _, inner)] =
        nodes
    else {
        return;
    };
    // Only into text the change starts with, so the space keeps a format.
    let Some(Node::Leaf(Leaf::Text { text: first, .. })) = inner.first_mut() else {
        return;
    };
    let kept = text.trim_end().len();
    if kept == text.len() || kept == 0 {
        return;
    }
    first.insert_str(0, &text[kept..]);
    text.truncate(kept);
}

/// Removes every comment below `nodes` into `comments`, in order.
fn lift_comments(nodes: Vec<Node>, comments: &mut Vec<Node>) -> Vec<Node> {
    let mut kept = Vec::new();
    for node in nodes {
        match node {
            Node::Leaf(Leaf::Comment(_)) => comments.push(node),
            Node::Span(mark, by, nodes) => {
                kept.push(Node::Span(mark, by, lift_comments(nodes, comments)));
            }
            leaf => kept.push(leaf),
        }
    }
    kept
}
