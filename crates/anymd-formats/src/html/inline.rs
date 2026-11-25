//! Inline Markdown, links and images, and whitespace handling.

use ego_tree::NodeRef;
use scraper::Node;
use url::Url;

use crate::images::Embed;

use super::extract::{element_name, is_dropped, BLOCK_TAGS};
use super::{Ctx, MAX_DEPTH};

fn render_inline_children(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, buf: &mut String, depth: usize) {
    for child in node.children() {
        if ctx.failure.borrow().is_some() {
            break;
        }
        render_inline(child, ctx, buf, depth + 1);
        if let Err(error) = crate::limits::check_output(buf) {
            *ctx.failure.borrow_mut() = Some(error);
            break;
        }
    }
}

pub(super) fn inline_string(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, depth: usize) -> String {
    // The sentinel keeps a leading space significant ("a<b> b</b>" → "a **b**").
    let mut buf = String::from("\u{1}");
    render_inline_children(node, ctx, &mut buf, depth);
    buf.split_off(1)
}

pub(super) fn render_inline(
    node: NodeRef<'_, Node>,
    ctx: &Ctx<'_>,
    buf: &mut String,
    depth: usize,
) {
    match node.value() {
        Node::Text(text) => {
            push_text(buf, text);
            return;
        }
        Node::Element(_) => {}
        _ => return,
    }
    if is_dropped(node, ctx) {
        return;
    }
    if depth > MAX_DEPTH {
        push_text(buf, &plain_text(node));
        return;
    }
    let Some(el) = node.value().as_element() else {
        return;
    };
    match el.name() {
        "br" => {
            trim_trailing_space(buf);
            buf.push('\n');
        }
        "img" => {
            if let Some(image) = render_image(el, ctx) {
                push_atom(buf, &image);
            }
        }
        "a" => render_link(node, ctx, buf, depth),
        "strong" | "b" => wrap(buf, &inline_string(node, ctx, depth), "**"),
        "em" | "i" => wrap(buf, &inline_string(node, ctx, depth), "*"),
        "del" | "s" | "strike" => wrap(buf, &inline_string(node, ctx, depth), "~~"),
        "code" | "kbd" | "samp" | "tt" => {
            let text = collapse(&plain_text(node));
            if !text.is_empty() {
                let fence = "`".repeat(longest_run(&text, '`') + 1);
                let pad = if text.starts_with('`') || text.ends_with('`') {
                    " "
                } else {
                    ""
                };
                push_atom(buf, &format!("{fence}{pad}{text}{pad}{fence}"));
            }
        }
        "math" => {
            if let Some(tex) = el.attr("alttext").map(clean_tex).filter(|t| !t.is_empty()) {
                push_atom(buf, &format!("${tex}$"));
            } else {
                push_text(buf, &plain_text(node));
            }
        }
        "q" => wrap(buf, &inline_string(node, ctx, depth), "\""),
        name if BLOCK_TAGS.contains(&name) => {
            push_text(buf, " ");
            render_inline_children(node, ctx, buf, depth);
            push_text(buf, " ");
        }
        _ => render_inline_children(node, ctx, buf, depth),
    }
}

fn render_link(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, buf: &mut String, depth: usize) {
    let inner = inline_string(node, ctx, depth);
    let text = inner.split_whitespace().collect::<Vec<_>>().join(" ");
    let href = node
        .value()
        .as_element()
        .and_then(|el| el.attr("href"))
        .map(str::trim)
        .unwrap_or("");
    let target = if href.is_empty()
        || href.starts_with('#')
        || href.to_ascii_lowercase().starts_with("javascript:")
    {
        None
    } else {
        resolve_url(href, ctx)
    };
    if text.starts_with("![") && text.ends_with(')') && text.matches("![").count() == 1 {
        // A linked image (thumbnail → file page): the image alone says it.
        push_atom(buf, &text);
        return;
    }
    let lead = inner.starts_with([' ', '\n']);
    let trail = inner.ends_with([' ', '\n']);
    if lead {
        push_text(buf, " ");
    }
    match target {
        _ if text.is_empty() => {}
        Some(url) if url == text || url.strip_prefix("mailto:") == Some(text.as_str()) => {
            push_atom(buf, &url)
        }
        Some(url) => {
            push_atom(
                buf,
                &format!("[{}]({})", bracket_safe(&text), markdown_url(&url)),
            );
        }
        None => push_atom(buf, &text),
    }
    if trail {
        push_text(buf, " ");
    }
}

fn render_image(el: &scraper::node::Element, ctx: &Ctx<'_>) -> Option<String> {
    let alt = collapse(el.attr("alt").unwrap_or(""));
    let class = el.attr("class").unwrap_or("");
    if !alt.is_empty() && class.contains("mwe-math-fallback") {
        // MediaWiki renders formulas as images whose alt text is the TeX source.
        return Some(format!("${}$", clean_tex(&alt)));
    }
    let tiny = |attr: &str| {
        el.attr(attr)
            .and_then(|v| v.trim().trim_end_matches("px").parse::<u32>().ok())
            .is_some_and(|v| v < 48)
    };
    if tiny("width") || tiny("height") {
        // Icons and badges carry no content.
        return None;
    }
    let src = [
        el.attr("src"),
        el.attr("data-src"),
        el.attr("data-original"),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|s| !s.is_empty() && !s.starts_with("data:"));
    if let (Some(decide), Some(src)) = (ctx.options.images, src) {
        match decide(src, &alt) {
            Embed::Markdown(markdown) => return Some(markdown),
            Embed::Skip => return None,
            Embed::Fallback => {}
        }
    }
    if alt.is_empty() {
        return None;
    }
    let alt = bracket_safe(&alt);
    // Unresolvable image paths stay as written (EPUB images live in the archive).
    Some(
        match src.map(|s| resolve_url(s, ctx).unwrap_or_else(|| s.to_string())) {
            Some(url) => format!("![{alt}]({})", markdown_url(&url)),
            None => format!("![{alt}]()"),
        },
    )
}

fn resolve_url(href: &str, ctx: &Ctx<'_>) -> Option<String> {
    if let Ok(url) = Url::parse(href) {
        return Some(strip_tracking(url));
    }
    if let Some(base) = &ctx.options.base {
        if let Ok(url) = base.join(href) {
            return Some(strip_tracking(url));
        }
    }
    ctx.options.keep_relative_links.then(|| href.to_string())
}

fn markdown_url(url: &str) -> String {
    let opens = url.matches('(').count();
    let closes = url.matches(')').count();
    if url.contains([' ', '<', '>']) || opens != closes {
        format!(
            "<{}>",
            url.replace(' ', "%20")
                .replace('<', "%3C")
                .replace('>', "%3E")
        )
    } else {
        url.to_string()
    }
}

/// Link text only needs escaping when its brackets are unbalanced.
fn bracket_safe(text: &str) -> String {
    let mut depth: i32 = 0;
    for c in text.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            break;
        }
    }
    if depth == 0 {
        text.to_string()
    } else {
        text.replace('[', "\\[").replace(']', "\\]")
    }
}

/// Drop tracking parameters (`utm_*`, `fbclid`, `gclid`) that only cost tokens.
fn strip_tracking(mut url: Url) -> String {
    if url.query().is_some() {
        let kept: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| !(k.starts_with("utm_") || k == "fbclid" || k == "gclid"))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if kept.is_empty() {
            url.set_query(None);
        } else if kept.len() != url.query_pairs().count() {
            url.query_pairs_mut().clear().extend_pairs(kept);
        }
    }
    url.to_string()
}

fn clean_tex(tex: &str) -> String {
    let tex = tex.trim();
    let inner = tex
        .strip_prefix("{\\displaystyle")
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or(tex);
    inner.trim().to_string()
}

pub(super) fn plain_text(node: NodeRef<'_, Node>) -> String {
    let mut out = String::new();
    for n in node.descendants() {
        if let Node::Text(text) = n.value() {
            if !n
                .ancestors()
                .take(4)
                .any(|a| matches!(element_name(a), Some("script" | "style")))
            {
                out.push_str(text);
            }
        }
    }
    out
}

pub(super) fn text_len(node: NodeRef<'_, Node>) -> usize {
    let mut len = 0;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        match n.value() {
            Node::Text(text) => len += text.trim().len(),
            Node::Element(el)
                if matches!(
                    el.name(),
                    "script" | "style" | "noscript" | "template" | "svg"
                ) => {}
            _ => stack.extend(n.children()),
        }
    }
    len
}

pub(super) fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn longest_run(text: &str, ch: char) -> usize {
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
pub(super) fn push_text(buf: &mut String, text: &str) {
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
fn push_atom(buf: &mut String, atom: &str) {
    buf.push_str(atom);
}

fn trim_trailing_space(buf: &mut String) {
    while buf.ends_with(' ') {
        buf.pop();
    }
}

fn wrap(buf: &mut String, inner: &str, marker: &str) {
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

pub(super) fn finish_inline(inline: &str) -> String {
    let lines: Vec<&str> = inline
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

/// Final cleanup: no trailing spaces, no runs of blank lines (outside code fences).
pub(super) fn tidy(markdown: &str) -> String {
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
