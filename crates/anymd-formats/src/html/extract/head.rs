//! Collect page metadata and resolve the document base URL.

use scraper::Html;
use url::Url;

use super::super::inline::{collapse, plain_text};
use super::HeadFacts;

pub(in crate::html) fn head_facts(doc: &Html) -> HeadFacts {
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

pub(in crate::html) fn resolve_base(
    base_url: Option<&str>,
    base_href: Option<&str>,
) -> Option<Url> {
    let base = base_url.and_then(|b| Url::parse(b).ok());
    match (base, base_href) {
        (Some(base), Some(href)) => base.join(href).ok().or(Some(base)),
        (Some(base), None) => Some(base),
        (None, Some(href)) => Url::parse(href).ok(),
        (None, None) => None,
    }
}
