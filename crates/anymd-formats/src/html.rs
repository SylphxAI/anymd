//! HTML → Markdown tuned for agents: main content only, no chrome, few tokens.
//!
//! A purpose-built walker over `scraper`/html5ever rather than a generic
//! turndown port: it needs main-content selection, noise pruning by role and
//! class, relative-URL resolution, and pipe tables built from block-level
//! cells, which a generic rule set does not give.

use ego_tree::NodeRef;
use scraper::{Html, Node};
use url::Url;

use crate::{ConvertError, Converted, Options, Section};

/// Deeper trees than this are flattened to text so recursion stays bounded.
const MAX_DEPTH: usize = 256;

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    let text = decode(bytes);
    let doc = Html::parse_document(&expand_self_closing(&text));
    let head = head_facts(&doc);
    let base = resolve_base(options.base_url.as_deref(), head.base_href.as_deref());
    let markdown = render_document(
        &doc,
        &RenderOptions {
            base,
            keep_relative_links: true,
            select_main: true,
        },
    );
    let mut metadata = Vec::new();
    if let Some(description) = head.description {
        metadata.push(("description".to_string(), description));
    }
    if let Some(author) = head.author {
        metadata.push(("author".to_string(), author));
    }
    if let Some(published) = head.published {
        metadata.push(("published".to_string(), published));
    }
    Ok(Converted {
        format: "html".into(),
        title: head.title,
        sections: vec![Section {
            label: "document".into(),
            markdown,
        }],
        metadata,
    })
}

/// Rendering knobs shared with the EPUB converter.
pub(crate) struct RenderOptions {
    pub base: Option<Url>,
    /// Keep links whose target is relative and cannot be resolved (EPUB drops them:
    /// they point into the archive and only cost tokens).
    pub keep_relative_links: bool,
    /// Narrow to `<main>`/`<article>` when the page has one.
    pub select_main: bool,
}

/// Markdown for an already-decoded HTML/XHTML document.
pub(crate) fn render_html(text: &str, options: &RenderOptions) -> String {
    render_document(&Html::parse_document(&expand_self_closing(text)), options)
}

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// XHTML allows `<a id="x"/>`; an HTML parser reads that as an open `<a>` that
/// swallows the rest of the page. Rewrite non-void self-closing tags as pairs.
fn expand_self_closing(text: &str) -> std::borrow::Cow<'_, str> {
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

fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

#[derive(Default)]
struct HeadFacts {
    title: Option<String>,
    description: Option<String>,
    author: Option<String>,
    published: Option<String>,
    base_href: Option<String>,
}

fn head_facts(doc: &Html) -> HeadFacts {
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

fn resolve_base(base_url: Option<&str>, base_href: Option<&str>) -> Option<Url> {
    let base = base_url.and_then(|b| Url::parse(b).ok());
    match (base, base_href) {
        (Some(base), Some(href)) => base.join(href).ok().or(Some(base)),
        (Some(base), None) => Some(base),
        (None, Some(href)) => Url::parse(href).ok(),
        (None, None) => None,
    }
}

fn render_document(doc: &Html, options: &RenderOptions) -> String {
    let root = doc.tree.root();
    let body = root
        .descendants()
        .find(|n| element_name(*n) == Some("body"))
        .unwrap_or(root);
    let content = if options.select_main {
        select_main(body)
    } else {
        body
    };
    let ctx = Ctx {
        options,
        root_text: text_len(content).max(1),
        root: content.id(),
    };
    let mut out = Blocks::default();
    render_block(content, &ctx, &mut out, 0);
    tidy(&out.finish("\n\n"))
}

// ---------------------------------------------------------------------------
// Main-content selection

/// Prefer `<main>`, `role=main`, a dominant `<article>`, or a well-known content
/// container; fall back to `<body>` with noise pruning.
fn select_main(body: NodeRef<'_, Node>) -> NodeRef<'_, Node> {
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

// ---------------------------------------------------------------------------
// Rendering

struct Ctx<'a> {
    options: &'a RenderOptions,
    root_text: usize,
    root: ego_tree::NodeId,
}

#[derive(Default)]
struct Blocks {
    blocks: Vec<String>,
    inline: String,
}

impl Blocks {
    fn flush(&mut self) {
        let paragraph = finish_inline(&self.inline);
        self.inline.clear();
        if !paragraph.is_empty() {
            self.blocks.push(paragraph);
        }
    }

    fn push(&mut self, block: String) {
        self.flush();
        if !block.trim().is_empty() {
            self.blocks.push(block);
        }
    }

    fn finish(mut self, separator: &str) -> String {
        self.flush();
        self.blocks.join(separator)
    }
}

fn element_name<'a>(node: NodeRef<'a, Node>) -> Option<&'a str> {
    node.value().as_element().map(|e| e.name())
}

const DROP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "canvas", "iframe", "object", "embed",
    "button", "input", "select", "textarea", "option", "head", "meta", "link", "map", "audio",
    "video", "source", "track", "dialog", "nav", "aside", "footer", "menu", "title",
];

const BLOCK_TAGS: &[&str] = &[
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

fn is_block(node: NodeRef<'_, Node>) -> bool {
    element_name(node).is_some_and(|name| BLOCK_TAGS.contains(&name))
}

fn is_dropped(node: NodeRef<'_, Node>, ctx: &Ctx<'_>) -> bool {
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

fn render_children(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, out: &mut Blocks, depth: usize) {
    for child in node.children() {
        render_block(child, ctx, out, depth + 1);
    }
}

fn render_block(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, out: &mut Blocks, depth: usize) {
    match node.value() {
        Node::Text(text) => {
            push_text(&mut out.inline, text);
            return;
        }
        Node::Element(_) => {}
        Node::Document | Node::Fragment => {
            render_children(node, ctx, out, depth);
            return;
        }
        _ => return,
    }
    if is_dropped(node, ctx) {
        return;
    }
    if depth > MAX_DEPTH {
        push_text(&mut out.inline, &plain_text(node));
        return;
    }
    let name = element_name(node).unwrap_or("");
    match name {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = usize::from(name.as_bytes()[1] - b'0');
            let text = inline_string(node, ctx, depth).replace('\n', " ");
            let text = text.trim();
            out.push(if text.is_empty() {
                String::new()
            } else {
                format!("{} {}", "#".repeat(level), text)
            });
        }
        "ul" | "ol" => {
            let list = render_list(node, ctx, depth);
            out.push(list);
        }
        "pre" => out.push(render_pre(node)),
        "blockquote" => {
            let mut inner = Blocks::default();
            render_children(node, ctx, &mut inner, depth);
            let inner = inner.finish("\n\n");
            let quoted = inner
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            out.push(quoted);
        }
        "table" => render_table(node, ctx, out, depth),
        "hr" => out.push("---".into()),
        "br" => out.inline.push('\n'),
        "dt" | "summary" => {
            let text = inline_string(node, ctx, depth);
            let text = text.trim();
            out.push(if text.is_empty() {
                String::new()
            } else {
                format!("**{text}**")
            });
        }
        _ if BLOCK_TAGS.contains(&name) => {
            out.flush();
            render_children(node, ctx, out, depth);
            out.flush();
        }
        "span" | "font" | "label" | "small" | "big" | "ins" | "mark"
            if node.descendants().take(2000).skip(1).any(is_block) =>
        {
            render_children(node, ctx, out, depth);
        }
        _ if name.contains('-') && node.descendants().take(2000).skip(1).any(is_block) => {
            // Custom elements are usually block wrappers.
            out.flush();
            render_children(node, ctx, out, depth);
            out.flush();
        }
        _ => render_inline(node, ctx, &mut out.inline, depth),
    }
}

fn render_inline_children(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, buf: &mut String, depth: usize) {
    for child in node.children() {
        render_inline(child, ctx, buf, depth + 1);
    }
}

fn inline_string(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, depth: usize) -> String {
    // The sentinel keeps a leading space significant ("a<b> b</b>" → "a **b**").
    let mut buf = String::from("\u{1}");
    render_inline_children(node, ctx, &mut buf, depth);
    buf.split_off(1)
}

fn render_inline(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, buf: &mut String, depth: usize) {
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
    if alt.is_empty() {
        return None;
    }
    let class = el.attr("class").unwrap_or("");
    if class.contains("mwe-math-fallback") {
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

fn render_list(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, depth: usize) -> String {
    let ordered = element_name(node) == Some("ol");
    let mut number: i64 = node
        .value()
        .as_element()
        .and_then(|el| el.attr("start"))
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1);
    let mut items: Vec<String> = Vec::new();
    for child in node.children() {
        if is_dropped(child, ctx) {
            continue;
        }
        match element_name(child) {
            Some("li") => {
                let mut inner = Blocks::default();
                render_children(child, ctx, &mut inner, depth + 1);
                let content = inner.finish("\n");
                if content.trim().is_empty() {
                    continue;
                }
                let marker = if ordered {
                    format!("{number}. ")
                } else {
                    "- ".to_string()
                };
                number += 1;
                items.push(indent_item(&marker, &content));
            }
            Some("ul" | "ol") => {
                // Invalid but common: a nested list directly inside a list.
                let nested = render_list(child, ctx, depth + 1);
                if nested.is_empty() {
                    continue;
                }
                let nested = indent_item("  ", &nested);
                match items.last_mut() {
                    Some(last) => {
                        last.push('\n');
                        last.push_str(&nested);
                    }
                    None => items.push(nested),
                }
            }
            _ => {
                let mut inner = Blocks::default();
                render_block(child, ctx, &mut inner, depth + 1);
                let content = inner.finish("\n");
                if !content.trim().is_empty() {
                    items.push(indent_item("- ", &content));
                }
            }
        }
    }
    items.join("\n")
}

fn indent_item(marker: &str, content: &str) -> String {
    let pad = " ".repeat(marker.len());
    let mut out = String::new();
    for (index, line) in content.lines().enumerate() {
        if index == 0 {
            out.push_str(marker);
            out.push_str(line);
        } else {
            out.push('\n');
            if !line.is_empty() {
                out.push_str(&pad);
                out.push_str(line);
            }
        }
    }
    out
}

fn render_pre(node: NodeRef<'_, Node>) -> String {
    let mut text = String::new();
    collect_pre_text(node, &mut text, 0);
    let text = text
        .strip_prefix('\n')
        .unwrap_or(&text)
        .trim_end()
        .to_string();
    if text.trim().is_empty() {
        return String::new();
    }
    let lang = code_language(node).unwrap_or_default();
    let fence = "`".repeat(longest_run(&text, '`').max(2) + 1);
    format!("{fence}{lang}\n{text}\n{fence}")
}

fn collect_pre_text(node: NodeRef<'_, Node>, out: &mut String, depth: usize) {
    for child in node.children() {
        match child.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(el) if el.name() == "br" => out.push('\n'),
            Node::Element(el) if matches!(el.name(), "script" | "style") => {}
            Node::Element(_) if depth < MAX_DEPTH => collect_pre_text(child, out, depth + 1),
            _ => {}
        }
    }
}

fn code_language(pre: NodeRef<'_, Node>) -> Option<String> {
    let mut candidates: Vec<NodeRef<'_, Node>> = vec![pre];
    candidates.extend(pre.children().filter(|c| element_name(*c) == Some("code")));
    candidates.extend(pre.parent());
    candidates.extend(pre.parent().and_then(|p| p.parent()));
    for node in candidates {
        let Some(el) = node.value().as_element() else {
            continue;
        };
        for attr in ["data-lang", "data-language"] {
            if let Some(lang) = el.attr(attr).map(str::trim).filter(|l| is_lang(l)) {
                return Some(lang.to_ascii_lowercase());
            }
        }
        for class in el.attr("class").unwrap_or("").split_whitespace() {
            for prefix in [
                "language-",
                "lang-",
                "highlight-source-",
                "mw-highlight-lang-",
                "highlight-",
            ] {
                if let Some(lang) = class.strip_prefix(prefix).filter(|l| is_lang(l)) {
                    return Some(lang.to_ascii_lowercase());
                }
            }
        }
    }
    None
}

fn is_lang(value: &str) -> bool {
    !matches!(
        value.to_ascii_lowercase().as_str(),
        "plain" | "plaintext" | "text" | "none" | "nohighlight" | "txt"
    ) && !value.is_empty()
        && value.len() <= 20
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_' | '.'))
}

fn render_table(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, out: &mut Blocks, depth: usize) {
    let nested = node
        .descendants()
        .skip(1)
        .any(|n| element_name(n) == Some("table"));
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut header_row = false;
    let mut caption = String::new();
    if !nested {
        for (section, row) in table_rows(node) {
            let mut cells = Vec::new();
            let mut all_th = true;
            for cell in row.children() {
                let Some(name) = element_name(cell) else {
                    continue;
                };
                if name != "td" && name != "th" {
                    continue;
                }
                if is_dropped(cell, ctx) {
                    continue;
                }
                all_th &= name == "th";
                let mut inner = Blocks::default();
                render_children(cell, ctx, &mut inner, depth + 2);
                cells.push(cell_text(&inner.finish("\n")));
                let span: usize = cell
                    .value()
                    .as_element()
                    .and_then(|el| el.attr("colspan"))
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(1);
                for _ in 1..span.clamp(1, 50) {
                    cells.push(String::new());
                }
            }
            if cells.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            if rows.is_empty() {
                header_row = section == "thead" || all_th;
            }
            rows.push(cells);
        }
        for child in node.children() {
            if element_name(child) == Some("caption") {
                caption = collapse(&inline_string(child, ctx, depth));
            }
        }
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if nested || width <= 1 || (rows.len() <= 1 && !header_row) {
        // Layout table: render cells as ordinary blocks.
        out.flush();
        render_children(node, ctx, out, depth);
        out.flush();
        return;
    }
    if !caption.is_empty() {
        out.push(caption);
    }
    let table = crate::markdown_table(&rows);
    out.push(table.trim_end().to_string());
}

/// Flatten block content into one table cell; list items become a comma list.
fn cell_text(content: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut listy = false;
    for line in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let stripped = line.strip_prefix("- ").or_else(|| {
            let digits = line.chars().take_while(char::is_ascii_digit).count();
            (digits > 0)
                .then(|| line[digits..].strip_prefix(". "))
                .flatten()
        });
        listy |= stripped.is_some();
        parts.push(stripped.unwrap_or(line));
    }
    parts.join(if listy { ", " } else { " " })
}

fn table_rows(table: NodeRef<'_, Node>) -> Vec<(&'static str, NodeRef<'_, Node>)> {
    let mut rows = Vec::new();
    for child in table.children() {
        match element_name(child) {
            Some("tr") => rows.push(("tbody", child)),
            Some(section @ ("thead" | "tbody" | "tfoot")) => {
                let section: &'static str = match section {
                    "thead" => "thead",
                    "tfoot" => "tfoot",
                    _ => "tbody",
                };
                rows.extend(
                    child
                        .children()
                        .filter(|r| element_name(*r) == Some("tr"))
                        .map(|r| (section, r)),
                );
            }
            _ => {}
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// Text helpers

fn plain_text(node: NodeRef<'_, Node>) -> String {
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

fn text_len(node: NodeRef<'_, Node>) -> usize {
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

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn longest_run(text: &str, ch: char) -> usize {
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
fn push_text(buf: &mut String, text: &str) {
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

fn finish_inline(inline: &str) -> String {
    let lines: Vec<&str> = inline
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

/// Final cleanup: no trailing spaces, no runs of blank lines (outside code fences).
fn tidy(markdown: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn md(html: &str) -> String {
        convert(html.as_bytes(), &Options::default())
            .unwrap()
            .sections[0]
            .markdown
            .clone()
    }

    #[test]
    fn extracts_article_and_drops_chrome() {
        let html = r#"<!doctype html><html><head><title>My Post</title>
            <meta name="description" content="A short post.">
            <script>var x = 1;</script><style>p{}</style></head>
            <body><header><a href="/">Logo</a><nav><a href="/a">Home</a></nav></header>
            <div class="cookie-banner">We use cookies. <button>OK</button></div>
            <main><article><h1>Hello   World</h1>
            <p>This is <strong>bold</strong> and <em>soft</em> text with a <a href="/docs/x">relative link</a>.
            Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore
            et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.</p>
            <ul><li>one</li><li>two<ul><li>nested</li></ul></li></ul>
            <ol start="3"><li>three</li><li>four</li></ol>
            <pre><code class="language-rust">fn main() {
    println!("hi");
}</code></pre>
            <img src="/a.png"><img src="b.png" alt="A chart">
            </article><aside>Related stuff</aside></main>
            <footer>Copyright</footer></body></html>"#;
        let converted = convert(
            html.as_bytes(),
            &Options {
                base_url: Some("https://example.com/blog/post".into()),
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(converted.title.as_deref(), Some("My Post"));
        assert_eq!(
            converted.metadata,
            vec![("description".into(), "A short post.".into())]
        );
        let md = &converted.sections[0].markdown;
        assert!(md.starts_with("# Hello World\n\nThis is **bold** and *soft* text with a [relative link](https://example.com/docs/x)."), "{md}");
        assert!(
            md.contains("- one\n- two\n  - nested\n\n3. three\n4. four"),
            "{md}"
        );
        assert!(
            md.contains("```rust\nfn main() {\n    println!(\"hi\");\n}\n```"),
            "{md}"
        );
        assert!(
            md.contains("![A chart](https://example.com/blog/b.png)"),
            "{md}"
        );
        for noise in [
            "Logo",
            "Home",
            "cookies",
            "Related",
            "Copyright",
            "var x",
            "a.png",
        ] {
            assert!(!md.contains(noise), "{noise} leaked: {md}");
        }
        assert!(!md.contains("\n\n\n"));
    }

    #[test]
    fn tables_become_pipe_tables() {
        let md = md("<table><caption>Scores</caption><thead><tr><th>Name</th><th>Score</th></tr></thead>\
             <tbody><tr><td>Ann <b>A</b></td><td>1|2</td></tr><tr><td colspan=2>total</td></tr></tbody></table>");
        assert_eq!(
            md,
            "Scores\n\n|Name|Score|\n|-|-|\n|Ann **A**|1\\|2|\n|total||"
        );
    }

    #[test]
    fn layout_tables_render_as_blocks() {
        let md = md("<table><tr><td><p>Only column</p></td></tr><tr><td>Second</td></tr></table>");
        assert_eq!(md, "Only column\n\nSecond");
    }

    #[test]
    fn inline_spacing_and_breaks() {
        assert_eq!(
            md("<p>a<b> b </b>c<br>d <code>x`y</code></p>"),
            "a **b** c\nd ``x`y``"
        );
        assert_eq!(
            md("<p><a href='#top'>Top</a> <a href='javascript:void(0)'>JS</a></p>"),
            "Top JS"
        );
        assert_eq!(
            md("<blockquote><p>q1</p><p>q2</p></blockquote>"),
            "> q1\n>\n> q2"
        );
    }

    #[test]
    fn hidden_and_noise_classes_are_dropped() {
        let body =
            "Real content paragraph that is long enough to dominate the page text. ".repeat(5);
        let html = format!(
            "<div class='sidebar'>Side</div><div style='display: none'>Hidden</div>\
             <div aria-hidden='true'>Aria</div><div role='navigation'>Menu</div><p>{body}</p>"
        );
        let md = md(&html);
        assert!(md.starts_with("Real content"), "{md}");
        assert!(
            !md.contains("Side")
                && !md.contains("Hidden")
                && !md.contains("Aria")
                && !md.contains("Menu")
        );
    }

    #[test]
    fn xhtml_self_closing_anchors_do_not_swallow_content() {
        let md = md("<?xml version='1.0'?><html><body><h2><a id=\"c1\"/>One<br/>Two</h2><p>A</p><p>B<span class='x'/></p></body></html>");
        assert_eq!(md, "## One Two\n\nA\n\nB");
    }

    #[test]
    fn malformed_input_never_panics() {
        for input in [
            "",
            "<",
            "<<<>>>",
            "<table><tr><td colspan=999999>x",
            "<pre>",
            "\u{0}\u{ffff}",
            "<a href>",
            "<a/",
            "<a b='/>",
            "<é/>",
        ] {
            let _ = md(input);
        }
        let deep = "<div>".repeat(5000) + "deep" + &"</div>".repeat(5000);
        assert!(md(&deep).contains("deep"));
        let _ = convert(&[0xff, 0xfe, 0x00, 0x3c], &Options::default()).unwrap();
    }
}
