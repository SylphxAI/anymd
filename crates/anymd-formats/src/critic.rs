//! CriticMarkup for tracked changes and comments.
//!
//! Converters record insertions, deletions, highlights and comments as flat
//! tokens in document order. [`Critic::into_inline`] then turns them into
//! markup that nests cleanly, so the simple regular expressions CriticMarkup is
//! designed for can parse it:
//!
//! - a span that crosses another is split so every span closes inside its parent;
//! - a span inside another of the same kind is merged into it (`{--{--x--}--}`
//!   would end at the first `--}`);
//! - spans with no text are dropped, keeping any comments they held;
//! - neighbouring spans of the same kind are joined;
//! - comments inside a highlight move to right after it (`{==text==}{>>note<<}`);
//! - a deletion next to an insertion becomes a substitution (`{~~old~>new~~}`).
//!
//! An insertion or deletion can carry who made it and when. That attribution
//! follows the change as a comment, which is how the CriticMarkup spec tracks
//! several authors: `{++new++}{>>Ana Lima (2026-09-29T14:05:00Z)<<}`. Neighbours
//! join only when their attributions match.
//!
//! Document text that happens to contain a CriticMarkup delimiter is escaped
//! with a Markdown backslash (`{\++`), which renders as the original characters
//! but no longer matches the delimiter.

mod join;
mod paragraph;
mod render;
mod spans;

pub(crate) use join::{join_changed, join_marked, splice, splice_len, Marked, Part};
pub(crate) use paragraph::Critic;
pub(crate) use render::escape;

/// A CriticMarkup span kind. Moves are an insertion at their destination and a
/// deletion at their source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    Insertion,
    Deletion,
    Highlight,
}

impl Mark {
    pub(crate) fn delimiters(self) -> (&'static str, &'static str) {
        match self {
            Self::Insertion => ("{++", "++}"),
            Self::Deletion => ("{--", "--}"),
            Self::Highlight => ("{==", "==}"),
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// A tracked change: its kind and who made it when, as the inside of its
/// `{>>…<<}` note (`Ana Lima (2026-09-29T14:05:00Z)`), already escaped.
pub(crate) type Change = (Mark, Option<String>);

fn note(inner: &str) -> String {
    format!("{{>>{inner}<<}}")
}

#[cfg(test)]
mod tests;
