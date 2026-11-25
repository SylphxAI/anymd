use super::Span;

mod emphasis;

use emphasis::render_emphasis;

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
