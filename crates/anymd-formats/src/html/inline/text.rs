//! Text extraction, measurement, and whitespace normalization.

use ego_tree::NodeRef;
use scraper::Node;

use super::super::extract::element_name;

pub(in crate::html) fn plain_text(node: NodeRef<'_, Node>) -> String {
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

pub(in crate::html) fn text_len(node: NodeRef<'_, Node>) -> usize {
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

pub(in crate::html) fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
