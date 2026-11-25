//! Preformatted text and Markdown fence language detection.

use ego_tree::NodeRef;
use scraper::Node;

use super::super::extract::element_name;
use super::super::inline::longest_run;
use super::super::MAX_DEPTH;

pub(super) fn render_pre(node: NodeRef<'_, Node>) -> String {
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
