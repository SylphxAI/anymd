/// ATX and Setext headings, excluding fenced code and indented code.
pub fn headings(markdown: &str) -> Vec<(usize, String, usize)> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut previous: Option<(&str, usize)> = None;
    let mut offset = 0;
    let mut comment = false;
    for line in markdown.split_inclusive('\n') {
        let text = line.trim_end();
        let trimmed = text.trim_start();
        let indent = text.len() - trimmed.len();
        let first = trimmed.chars().next().unwrap_or(' ');
        let run = trimmed.chars().take_while(|&c| c == first).count();
        if fence.is_none() && (comment || trimmed.starts_with("<!--")) {
            comment = !trimmed.contains("-->");
            previous = None;
            offset += line.len();
            continue;
        }
        if indent <= 3 && matches!(first, '`' | '~') && run >= 3 {
            if let Some((ch, length)) = fence {
                if first == ch && run >= length && trimmed[run..].trim().is_empty() {
                    fence = None;
                }
            } else {
                fence = Some((first, run));
            }
            previous = None;
        } else if fence.is_none() && indent <= 3 && !text.starts_with('\t') {
            if first == '#'
                && run <= 6
                && (trimmed.len() == run || trimmed.as_bytes()[run].is_ascii_whitespace())
            {
                let title = trimmed[run..].trim();
                let suffix = title.trim_end_matches('#');
                let title = if suffix.ends_with(char::is_whitespace) {
                    suffix.trim_end()
                } else {
                    title
                };
                if !title.is_empty() {
                    out.push((run, title.into(), offset));
                }
                previous = None;
            } else if matches!(first, '=' | '-') && run == trimmed.len() && run > 0 {
                if let Some((title, start)) = previous.take() {
                    out.push((if first == '=' { 1 } else { 2 }, title.into(), start));
                }
            } else {
                previous = (!trimmed.is_empty()).then_some((trimmed, offset));
            }
        } else {
            previous = None;
        }
        offset += line.len();
    }
    out
}
