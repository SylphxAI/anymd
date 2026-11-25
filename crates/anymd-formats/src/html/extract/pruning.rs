//! Drop page chrome and hidden elements without pruning the selected root.

use ego_tree::NodeRef;
use scraper::Node;

use super::super::inline::text_len;
use super::super::Ctx;
use super::elements::element_name;

const DROP_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "canvas", "iframe", "object", "embed",
    "button", "input", "select", "textarea", "option", "head", "meta", "link", "map", "audio",
    "video", "source", "track", "dialog", "nav", "aside", "footer", "menu", "title",
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

pub(in crate::html) fn is_dropped(node: NodeRef<'_, Node>, ctx: &Ctx<'_>) -> bool {
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
        // Keep headers with a headline; any nested nav is dropped separately.
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
