use crate::critic::{self, Change};
use crate::ConvertError;

/// Join Markdown blocks: list items stay tight, everything else gets a blank line.
pub(crate) struct Blocks {
    limit: usize,
    failure: Option<ConvertError>,
    out: String,
    last_was_list: bool,
    /// A tracked change on the break before the next block.
    separator: Option<Change>,
    /// The last paragraph ended with a space, which rendering trimmed.
    trailing_space: bool,
}

impl Blocks {
    pub(crate) fn new() -> Self {
        Self::with_limit(crate::limits::MAX_OUTPUT_BYTES)
    }

    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            out: String::new(),
            failure: None,
            last_was_list: false,
            separator: None,
            trailing_space: false,
        }
    }

    /// A block whose start cannot carry a CriticMarkup delimiter (a table, a
    /// text box); a tracked break before it is left unmarked.
    pub(crate) fn push(&mut self, block: &str, is_list: bool) {
        self.separator = None;
        self.push_prefixed("", block, is_list);
    }

    /// A block made of Markdown syntax (`## `, `  1. `) followed by its text. A
    /// tracked break before it closes after the syntax, so the heading or list
    /// item stays valid.
    pub(crate) fn push_prefixed(&mut self, prefix: &str, body: &str, is_list: bool) {
        self.push_paragraph(prefix, body, is_list, (false, false));
    }

    /// A paragraph, with whether its text started and ended with a space before
    /// rendering trimmed it (see [`critic::splice`]).
    pub(crate) fn push_paragraph(
        &mut self,
        prefix: &str,
        body: &str,
        is_list: bool,
        (leading, trailing): (bool, bool),
    ) {
        if self.is_full() {
            return;
        }
        let space = self.trailing_space || leading;
        let body = body.trim_end();
        if body.trim().is_empty() {
            return;
        }
        let change = self.separator.take();
        let separator = if is_list && self.last_was_list {
            "\n"
        } else {
            "\n\n"
        };
        let size = if self.out.is_empty() {
            prefix.len().saturating_add(body.len())
        } else {
            critic::splice_len(&self.out, separator, prefix, body, change.as_ref(), space)
        };
        if size > self.limit {
            self.failure = Some(ConvertError::limit(
                "output",
                crate::limits::MAX_OUTPUT_BYTES,
            ));
            return;
        }
        if self.out.is_empty() {
            self.out.push_str(prefix);
            self.out.push_str(body);
        } else {
            critic::splice(
                &mut self.out,
                separator,
                prefix,
                body,
                change.as_ref(),
                space,
            );
        }
        self.last_was_list = is_list;
        self.trailing_space = trailing;
    }

    /// Records that the paragraph mark ending the last block was inserted or
    /// deleted (`None` clears it).
    pub(crate) fn set_separator(&mut self, change: Option<Change>) {
        self.separator = change;
    }

    pub(crate) fn is_full(&self) -> bool {
        self.failure.is_some()
    }

    pub(crate) fn finish(self) -> Result<String, ConvertError> {
        self.failure.map_or(Ok(self.out), Err)
    }
}
