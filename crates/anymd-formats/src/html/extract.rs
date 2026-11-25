//! HTML/XHTML parser preparation, page metadata, and content selection.

use ego_tree::NodeRef;
use scraper::{Html, Node};
use url::Url;

use super::inline::{collapse, plain_text, text_len};
use super::Ctx;

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// XHTML allows `<a id="x"/>`; an HTML parser reads that as an open `<a>` that
/// swallows the rest of the page. Rewrite non-void self-closing tags as pairs.
pub(super) fn expand_self_closing(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("/>") {
        return std::borrow::Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 64);
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' || !bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        let name_end = (i + 1..bytes.len())
            .find(|&j| !(bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-' || bytes[j] == b':'))
            .unwrap_or(bytes.len());
        // Find the tag end, honouring quoted attribute values.
        let mut quote = 0u8;
        let mut end = None;
        for (j, &b) in bytes.iter().enumerate().skip(name_end) {
            match (quote, b) {
                (0, b'"' | b'\'') => quote = b,
                (0, b'>') => {
                    end = Some(j);
                    break;
                }
                (q, b) if q == b => quote = 0,
                _ => {}
            }
        }
        let Some(end) = end else { break };
        let name = &text[i + 1..name_end];
        let local = name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase();
        if bytes[end - 1] == b'/' && !VOID_TAGS.contains(&local.as_str()) {
            out.push_str(&text[copied..end - 1]);
            out.push_str("></");
            out.push_str(name);
            out.push('>');
            copied = end + 1;
        }
        i = end + 1;
    }
    out.push_str(&text[copied..]);
    std::borrow::Cow::Owned(out)
}

#[derive(Default)]
pub(super) struct HeadFacts {
    pub(super) title: Option<String>,
    pub(super) description: Option<String>,
    pub(super) author: Option<String>,
    pub(super) published: Option<String>,
    pub(super) base_href: Option<String>,
}

pub(super) fn head_facts(doc: &Html) -> HeadFacts {
    let mut facts = HeadFacts::default();
    let mut og_title = None;
    let mut og_description = None;
    for node in doc.tree.root().descendants() {
        let Some(el) = node.value().as_element() else {
            continue;
        };
        match el.name() {
            "title" if facts.title.is_none() => {
                let title = collapse(&plain_text(node));
                if !title.is_empty() {
                    facts.title = Some(title);
                }
            }
            "base" if facts.base_href.is_none() => {
                facts.base_href = el.attr("href").map(str::to_string);
            }
            "meta" => {
                let key = el
                    .attr("name")
                    .or_else(|| el.attr("property"))
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let Some(content) = el.attr("content").map(collapse).filter(|c| !c.is_empty())
                else {
                    continue;
                };
                match key.as_str() {
                    "description" => facts.description = facts.description.or(Some(content)),
                    "og:description" | "twitter:description" => {
                        og_description = og_description.or(Some(content))
                    }
                    "og:title" | "twitter:title" => og_title = og_title.or(Some(content)),
                    "author" | "article:author" => facts.author = facts.author.or(Some(content)),
                    "article:published_time" | "date" | "dc.date" => {
                        facts.published = facts.published.or(Some(content))
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    facts.title = facts.title.or(og_title);
    facts.description = facts.description.or(og_description);
    facts
}

pub(super) fn resolve_base(base_url: Option<&str>, base_href: Option<&str>) -> Option<Url> {
    let base = base_url.and_then(|b| Url::parse(b).ok());
    match (base, base_href) {
        (Some(base), Some(href)) => base.join(href).ok().or(Some(base)),
        (Some(base), None) => Some(base),
        (None, Some(href)) => Url::parse(href).ok(),
        (None, None) => None,
    }
}

/// Prefer `<main>`, `role=main`, a dominant `<article>`, or a well-known content
/// container; fall back to `<body>` with noise pruning.
pub(super) fn select_main(body: NodeRef<'_, Node>) -> NodeRef<'_, Node> {
    let body_len = text_len(body).max(1);
    let substantial = |node: NodeRef<'_, Node>| {
        let len = text_len(node);
        len >= 200 && len * 4 >= body_len
    };

    let mut mains = Vec::new();
    let mut articles = Vec::new();
    let mut known = Vec::new();
    for node in body.descendants() {
        let Some(el) = node.value().as_element() else {
            continue;
        };
        if el.name() == "main" || el.attr("role") == Some("main") {
            mains.push(node);
        } else if el.name() == "article" {
            articles.push(node);
        } else if let Some(id) = el.id() {
            if matches!(
                id,
                "content"
                    | "main-content"
                    | "maincontent"
                    | "mw-content-text"
                    | "article"
                    | "article-body"
            ) {
                known.push(node);
            }
        }
    }

    // A single dominant article beats a <main> that also wraps listings/related links.
    let article_total: usize = articles.iter().map(|a| text_len(*a)).sum();
    if let Some(best) = articles.iter().copied().max_by_key(|a| text_len(*a)) {
        if substantial(best) && text_len(best) * 10 >= article_total * 6 {
            return best;
        }
    }
    if let Some(main) = mains.into_iter().find(|m| substantial(*m)) {
        return main;
    }
    if let Some(node) = known.into_iter().find(|n| substantial(*n)) {
        return node;
    }
    body
}

pub(super) fn element_name<'a>(node: NodeRef<'a, Node>) -> Option<&'a str> {
    node.value().as_element().map(|e| e.name())
}

const DROP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "canvas", "iframe", "object", "embed",
    "button", "input", "select", "textarea", "option", "head", "meta", "link", "map", "audio",
    "video", "source", "track", "dialog", "nav", "aside", "footer", "menu", "title",
];

pub(super) const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "center",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "html",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

const NOISE_TOKENS: &[&str] = &[
    "nav",
    "navbar",
    "navbox",
    "navigation",
    "menu",
    "footer",
    "sidebar",
    "cookie",
    "cookies",
    "consent",
    "gdpr",
    "banner",
    "breadcrumb",
    "breadcrumbs",
    "share",
    "sharing",
    "social",
    "advert",
    "advertisement",
    "ads",
    "promo",
    "newsletter",
    "subscribe",
    "popup",
    "modal",
    "editsection",
    "ambox",
    "hatnote",
    "toolbar",
    "noprint",
    "catlinks",
    "printfooter",
    "related",
    "comments",
    "skip",
    "sronly",
    "srtext",
    "visuallyhidden",
    "screenreadertext",
];

const DROP_ROLES: &[&str] = &[
    "navigation",
    "banner",
    "contentinfo",
    "complementary",
    "search",
    "dialog",
    "alertdialog",
    "menu",
    "menubar",
];

pub(super) fn is_block(node: NodeRef<'_, Node>) -> bool {
    element_name(node).is_some_and(|name| BLOCK_TAGS.contains(&name))
}

pub(super) fn is_dropped(node: NodeRef<'_, Node>, ctx: &Ctx<'_>) -> bool {
    let Some(el) = node.value().as_element() else {
        return false;
    };
    if node.id() == ctx.root {
        return false;
    }
    let name = el.name();
    if DROP_TAGS.contains(&name) {
        return true;
    }
    if name == "header" {
        // Keep a header that carries the headline (its nav is dropped on its own); drop site headers. that carries the headline; drop site headers.
        let has_heading = node
            .descendants()
            .take(400)
            .any(|n| matches!(element_name(n), Some("h1" | "h2")));
        if !has_heading {
            return true;
        }
    }
    if el.attr("hidden").is_some() || el.attr("aria-hidden") == Some("true") {
        return true;
    }
    if let Some(style) = el.attr("style") {
        let style: String = style
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        if style.contains("display:none") || style.contains("visibility:hidden") {
            return true;
        }
    }
    if let Some(role) = el.attr("role") {
        if DROP_ROLES.contains(&role.to_ascii_lowercase().as_str()) {
            return true;
        }
    }
    let noisy = el
        .attr("class")
        .into_iter()
        .chain(el.id())
        .any(|value| value.split_whitespace().any(is_noise_name));
    // Never let a class heuristic eat the bulk of the page.
    noisy && text_len(node) * 10 < ctx.root_text * 4
}

fn is_noise_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let squashed: String = lower.chars().filter(|c| *c != '-' && *c != '_').collect();
    if NOISE_TOKENS.contains(&squashed.as_str()) {
        return true;
    }
    lower
        .split(['-', '_'])
        .any(|token| NOISE_TOKENS.contains(&token))
}
