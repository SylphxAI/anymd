//! Classify DOM elements for block and inline rendering.

use ego_tree::NodeRef;
use scraper::Node;

pub(in crate::html) fn element_name<'a>(node: NodeRef<'a, Node>) -> Option<&'a str> {
    node.value().as_element().map(|e| e.name())
}

pub(in crate::html) const BLOCK_TAGS: &[&str] = &[
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

pub(in crate::html) fn is_block(node: NodeRef<'_, Node>) -> bool {
    element_name(node).is_some_and(|name| BLOCK_TAGS.contains(&name))
}
