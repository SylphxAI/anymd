use crate::critic::{self, Change};
use crate::ConvertError;

mod media;

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

/// Accumulates spans for one paragraph.
#[derive(Debug, Default)]
pub(crate) struct Inline {
    spans: Vec<Span>,
}

impl Inline {
    /// Whether the text starts and ends with a space, which rendering trims.
    pub(crate) fn edges(&self) -> (bool, bool) {
        let spaced = |c: char| matches!(c, ' ' | '\u{a0}' | '\t');
        let mut texts = self
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .filter(|t| !t.is_empty());
        let leading = texts.next().is_some_and(|t| t.starts_with(spaced));
        let trailing = self
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .rfind(|t| !t.is_empty())
            .is_some_and(|t| t.ends_with(spaced));
        (leading, trailing)
    }

    pub(crate) fn push(&mut self, text: &str, bold: bool, italic: bool, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if last.bold == bold && last.italic == italic && last.link.as_deref() == link {
                last.text.push_str(text);
                return;
            }
        }
        self.spans.push(Span {
            text: text.to_string(),
            bold,
            italic,
            link: link.map(|name| name.to_string()),
        });
    }

    pub(crate) fn is_blank(&self) -> bool {
        self.spans.iter().all(|s| s.text.trim().is_empty())
    }

    /// Render to Markdown. `emphasis` false drops bold/italic (headings).
    pub(crate) fn render(&self, emphasis: bool) -> String {
        let mut out = String::new();
        let mut index = 0;
        while index < self.spans.len() {
            let link = self.spans[index].link.clone();
            let mut end = index + 1;
            while end < self.spans.len() && self.spans[end].link == link {
                end += 1;
            }
            let group = &self.spans[index..end];
            match link {
                Some(url) if !group.iter().all(|s| s.text.trim().is_empty()) => {
                    let inner = render_emphasis(group, emphasis);
                    let (lead, core, trail) = split_ws(&inner);
                    let label = core.replace('[', "\\[").replace(']', "\\]");
                    let url = url.replace(' ', "%20").replace(')', "%29");
                    if label == url
                        || (label.contains('@')
                            && label.trim_start_matches("mailto:")
                                == url.trim_start_matches("mailto:"))
                    {
                        out.push_str(&format!("{lead}<{url}>{trail}"));
                    } else {
                        out.push_str(&format!("{lead}[{label}]({url}){trail}"));
                    }
                }
                _ => out.push_str(&render_emphasis(group, emphasis)),
            }
            if out.len() > crate::limits::MAX_OUTPUT_BYTES {
                break;
            }
            index = end;
        }
        tidy_inline(&out)
    }
}

fn render_emphasis(spans: &[Span], emphasis: bool) -> String {
    let mut out = String::new();
    // Merge neighbours with identical emphasis (links already grouped).
    let mut merged: Vec<(bool, bool, String)> = Vec::new();
    for span in spans {
        let (bold, italic) = if emphasis {
            (span.bold, span.italic)
        } else {
            (false, false)
        };
        // Whitespace-only spans inherit the previous formatting so markers do not split.
        let (bold, italic) = if span.text.trim().is_empty() {
            merged
                .last()
                .map(|(b, i, _)| (*b, *i))
                .unwrap_or((bold, italic))
        } else {
            (bold, italic)
        };
        match merged.last_mut() {
            Some((b, i, text)) if *b == bold && *i == italic => text.push_str(&span.text),
            _ => merged.push((bold, italic, span.text.clone())),
        }
    }
    for (bold, italic, text) in merged {
        let (lead, core, trail) = split_ws(&text);
        if core.is_empty() || (!bold && !italic) {
            out.push_str(&text);
            continue;
        }
        let (open, close) = match (bold, italic) {
            (true, true) => ("**_", "_**"),
            (true, false) => ("**", "**"),
            _ => ("_", "_"),
        };
        out.push_str(lead);
        // Markers cannot wrap line breaks; apply them per line.
        let lines: Vec<String> = core
            .split('\n')
            .map(|line| {
                let (l, c, t) = split_ws(line);
                if c.is_empty() {
                    line.to_string()
                } else {
                    format!("{l}{open}{c}{close}{t}")
                }
            })
            .collect();
        out.push_str(&lines.join("\n"));
        out.push_str(trail);
    }
    out
}

fn split_ws(s: &str) -> (&str, &str, &str) {
    let start = s.len() - s.trim_start().len();
    let end = s.trim_end().len().max(start);
    (&s[..start], &s[start..end], &s[end..])
}

/// Collapse runs of spaces, trim line ends, and drop empty lines inside a paragraph.
fn tidy_inline(s: &str) -> String {
    let mut lines = Vec::new();
    for line in s.split('\n') {
        let mut out = String::with_capacity(line.len());
        let mut prev_space = false;
        for c in line.chars() {
            let is_space = c == ' ' || c == '\u{a0}';
            if is_space {
                if !prev_space {
                    out.push(' ');
                }
            } else {
                out.push(c);
            }
            prev_space = is_space;
        }
        lines.push(out.trim().to_string());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    lines.join("\n")
}

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

/// Tracks list marker widths so nested items indent under their parent's content.
#[derive(Default)]
pub(crate) struct ListIndent {
    widths: Vec<usize>,
}

impl ListIndent {
    pub(crate) fn reset(&mut self) {
        self.widths.clear();
    }

    /// Format an item at `level` with `marker` (e.g. "-", "3.").
    pub(crate) fn item(&mut self, level: usize, marker: &str, text: &str) -> String {
        let level = level.min(8);
        self.widths.truncate(level);
        while self.widths.len() < level {
            self.widths.push(2);
        }
        let indent = " ".repeat(self.widths.iter().sum());
        self.widths.push(marker.chars().count() + 1);
        let continuation = format!("\n{}", " ".repeat(self.widths.iter().sum()));
        format!("{indent}{marker} {}", text.replace('\n', &continuation))
    }
}
