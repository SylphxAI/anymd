mod blocks;
mod inline;
mod list_indent;
mod media;

pub(crate) use blocks::Blocks;
pub(crate) use inline::Inline;
pub(crate) use list_indent::ListIndent;
pub(crate) use media::{image_markdown, Media};

pub(crate) fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One formatted stretch of inline text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub link: Option<String>,
}
