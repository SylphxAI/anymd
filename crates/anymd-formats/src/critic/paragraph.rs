//! Records converter events and renders a single paragraph.

use super::render;
use super::spans::{space_into_last_change, tidy, tree, Leaf, Token};
use super::Mark;
use crate::ooxml::Inline;

/// Inline content of one paragraph with CriticMarkup spans.
#[derive(Debug, Default)]
pub(crate) struct Critic {
    tokens: Vec<Token>,
    /// Text written as is, for a document with no tracked change or comment:
    /// nothing can open a span there, so nothing needs escaping.
    plain: bool,
    /// The paragraph is joined to more text on the same line (a flattened
    /// table's cells): see [`space_into_last_change`].
    inline_end: bool,
}

impl Critic {
    /// A paragraph of a document with no tracked change or comment: its text
    /// is written unescaped.
    pub(crate) fn plain() -> Self {
        Self {
            plain: true,
            ..Self::default()
        }
    }

    /// Marks the paragraph as joined to more text on the same line.
    pub(crate) fn set_inline_end(&mut self) {
        self.inline_end = true;
    }

    pub(crate) fn push(&mut self, text: &str, bold: bool, italic: bool, link: Option<&str>) {
        if !text.is_empty() {
            self.tokens.push(Token::Leaf(Leaf::Text {
                text: text.to_string(),
                bold,
                italic,
                link: link.map(str::to_string),
            }));
        }
    }

    pub(crate) fn raw(&mut self, markdown: &str) {
        if !markdown.is_empty() {
            self.tokens
                .push(Token::Leaf(Leaf::Raw(markdown.to_string())));
        }
    }

    /// Converter-built Markdown at token position `at` (see [`Critic::len`]),
    /// as the start of the paragraph: text right after it loses its leading
    /// space, up to the first marker, as the paragraph's own start would.
    pub(crate) fn insert_raw(&mut self, at: usize, markdown: &str) {
        for token in &mut self.tokens[at..] {
            match token {
                Token::Leaf(Leaf::Text { text, .. }) => {
                    let trimmed = text.trim_start().to_string();
                    let done = !trimmed.is_empty();
                    *text = trimmed;
                    if done {
                        break;
                    }
                }
                _ => break,
            }
        }
        self.tokens
            .insert(at, Token::Leaf(Leaf::Raw(markdown.to_string())));
    }

    /// The number of tokens so far, a position for [`Critic::insert_raw`].
    pub(crate) fn len(&self) -> usize {
        self.tokens.len()
    }

    pub(crate) fn comment(&mut self, note: &str) {
        self.tokens
            .push(Token::Leaf(Leaf::Comment(note.to_string())));
    }

    /// Opens a span; `by` is the attribution of an insertion or deletion.
    pub(crate) fn open(&mut self, mark: Mark, by: Option<&str>) {
        self.tokens.push(Token::Open(mark, by.map(str::to_string)));
    }

    pub(crate) fn close(&mut self, mark: Mark) {
        self.tokens.push(Token::Close(mark));
    }

    /// Drops every attribution. Inside a comment's own text an attribution would
    /// put a `{>>…<<}` inside another, and the outer one would end at the inner
    /// closer.
    pub(crate) fn unattributed(mut self) -> Self {
        for token in &mut self.tokens {
            if let Token::Open(_, by) = token {
                *by = None;
            }
        }
        self
    }

    /// True when there is no visible text and no comment.
    pub(crate) fn is_blank(&self) -> bool {
        self.tokens.iter().all(|token| match token {
            Token::Leaf(Leaf::Text { text, .. } | Leaf::Raw(text)) => text.trim().is_empty(),
            Token::Leaf(Leaf::Comment(_)) => false,
            Token::Open(..) | Token::Close(_) => true,
        })
    }

    pub(crate) fn into_inline(self) -> Inline {
        let mut nodes = tidy(tree(self.tokens));
        if self.inline_end {
            space_into_last_change(&mut nodes);
        }
        render::inline(&nodes, self.plain)
    }
}
