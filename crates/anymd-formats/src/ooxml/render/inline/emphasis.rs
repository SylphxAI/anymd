use super::{split_ws, Span};

pub(super) fn render_emphasis(spans: &[Span], emphasis: bool) -> String {
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
