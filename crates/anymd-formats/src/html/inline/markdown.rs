//! Whitespace, delimiters, and final cleanup for rendered Markdown.

pub(super) fn clean_tex(tex: &str) -> String {
    let tex = tex.trim();
    let inner = tex
        .strip_prefix("{\\displaystyle")
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or(tex);
    inner.trim().to_string()
}

pub(in crate::html) fn longest_run(text: &str, ch: char) -> usize {
    let mut best = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == ch {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

/// Append text with HTML whitespace collapsing.
pub(in crate::html) fn push_text(buf: &mut String, text: &str) {
    for c in text.chars() {
        if c.is_whitespace() {
            if !buf.is_empty() && !buf.ends_with([' ', '\n']) {
                buf.push(' ');
            }
        } else {
            buf.push(c);
        }
    }
}

/// Append pre-rendered Markdown verbatim.
pub(in crate::html) fn push_atom(buf: &mut String, atom: &str) {
    buf.push_str(atom);
}

pub(in crate::html) fn trim_trailing_space(buf: &mut String) {
    while buf.ends_with(' ') {
        buf.pop();
    }
}

pub(in crate::html) fn wrap(buf: &mut String, inner: &str, marker: &str) {
    let core = inner.trim();
    if inner.starts_with([' ', '\n']) {
        push_text(buf, " ");
    }
    if !core.is_empty() {
        if core.contains('\n') {
            // Emphasis cannot span hard breaks in Markdown; keep the text plain.
            buf.push_str(core);
        } else {
            buf.push_str(marker);
            buf.push_str(core);
            buf.push_str(marker);
        }
    }
    if inner.ends_with([' ', '\n']) && !core.is_empty() {
        push_text(buf, " ");
    }
}

pub(in crate::html) fn finish_inline(inline: &str) -> String {
    let lines: Vec<&str> = inline
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

/// Final cleanup: no trailing spaces, no runs of blank lines (outside code fences).
pub(in crate::html) fn tidy(markdown: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    let mut fence: Option<String> = None;
    for line in markdown.lines() {
        let trimmed_start = line.trim_start();
        if let Some(open) = &fence {
            out.push_str(line);
            out.push('\n');
            if trimmed_start.trim_end() == open {
                fence = None;
            }
            continue;
        }
        if trimmed_start.starts_with("```") {
            let ticks: String = trimmed_start.chars().take_while(|c| *c == '`').collect();
            fence = Some(ticks);
        }
        let line = line.trim_end();
        if line.is_empty() {
            blank += 1;
            if blank > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}
