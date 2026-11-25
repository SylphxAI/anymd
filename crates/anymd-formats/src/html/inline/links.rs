//! Link and image Markdown, including destination resolution and escaping.

use ego_tree::NodeRef;
use scraper::Node;
use url::Url;

use crate::images::Embed;

use super::super::Ctx;
use super::markdown::{clean_tex, push_atom, push_text};
use super::text::collapse;

pub(super) fn render_link(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, buf: &mut String, inner: &str) {
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

pub(super) fn render_image(el: &scraper::node::Element, ctx: &Ctx<'_>) -> Option<String> {
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
