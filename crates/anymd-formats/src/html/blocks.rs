//! Block traversal, lists, code fences, and bounded Markdown accumulation.

use ego_tree::NodeRef;
use scraper::Node;

use crate::ConvertError;

use super::extract::{element_name, is_block, is_dropped, BLOCK_TAGS};
use super::inline::{
    finish_inline, inline_string, longest_run, plain_text, push_text, render_inline,
};
use super::{Ctx, MAX_DEPTH};

mod table;
use table::render_table;

#[derive(Default)]
pub(super) struct Blocks {
    blocks: Vec<String>,
    inline: String,
    bytes: usize,
    failure: Option<ConvertError>,
}

impl Blocks {
    fn flush(&mut self) {
        let paragraph = finish_inline(&self.inline);
        self.inline.clear();
        if !paragraph.is_empty() {
            self.push_checked(paragraph);
        }
    }

    fn push(&mut self, block: String) {
        self.flush();
        if !block.trim().is_empty() {
            self.push_checked(block);
        }
    }

    fn push_checked(&mut self, block: String) {
        if self.failure.is_some() {
            return;
        }
        self.bytes = self.bytes.saturating_add(block.len());
        let separators = self.blocks.len().saturating_mul(2);
        if self.bytes.saturating_add(separators) > crate::limits::MAX_OUTPUT_BYTES {
            self.failure = Some(ConvertError::limit(
                "output",
                crate::limits::MAX_OUTPUT_BYTES,
            ));
        } else {
            self.blocks.push(block);
        }
    }

    pub(super) fn finish(mut self, separator: &str) -> Result<String, ConvertError> {
        self.flush();
        if let Some(error) = self.failure {
            return Err(error);
        }
        Ok(self.blocks.join(separator))
    }
}

fn finish_blocks(blocks: Blocks, separator: &str, ctx: &Ctx<'_>) -> String {
    blocks.finish(separator).unwrap_or_else(|error| {
        *ctx.failure.borrow_mut() = Some(error);
        String::new()
    })
}

fn render_children(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, out: &mut Blocks, depth: usize) {
    for child in node.children() {
        render_block(child, ctx, out, depth + 1);
    }
}

pub(super) fn render_block(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, out: &mut Blocks, depth: usize) {
    if out.failure.is_some() || ctx.failure.borrow().is_some() {
        return;
    }
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
            let inner = finish_blocks(inner, "\n\n", ctx);
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

fn render_list(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, depth: usize) -> String {
    let ordered = element_name(node) == Some("ol");
    let mut number: i64 = node
        .value()
        .as_element()
        .and_then(|el| el.attr("start"))
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1);
    let mut items: Vec<String> = Vec::new();
    let mut bytes = 0usize;
    for child in node.children() {
        if ctx.failure.borrow().is_some() {
            break;
        }
        let previous = items.last().map_or(0, String::len);
        let count = items.len();
        if is_dropped(child, ctx) {
            continue;
        }
        match element_name(child) {
            Some("li") => {
                let mut inner = Blocks::default();
                render_children(child, ctx, &mut inner, depth + 1);
                let content = finish_blocks(inner, "\n", ctx);
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
                let content = finish_blocks(inner, "\n", ctx);
                if !content.trim().is_empty() {
                    items.push(indent_item("- ", &content));
                }
            }
        }
        let added = if items.len() > count {
            items.last().map_or(0, String::len).saturating_add(1)
        } else {
            items.last().map_or(0, String::len).saturating_sub(previous)
        };
        bytes = bytes.saturating_add(added);
        if bytes.saturating_sub(1) > crate::limits::MAX_OUTPUT_BYTES {
            *ctx.failure.borrow_mut() = Some(ConvertError::limit(
                "output",
                crate::limits::MAX_OUTPUT_BYTES,
            ));
            return String::new();
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
