//! List numbering, continuation indentation, and bounded list assembly.

use ego_tree::NodeRef;
use scraper::Node;

use crate::ConvertError;

use super::super::extract::{element_name, is_dropped};
use super::super::Ctx;
use super::{finish_blocks, render_block, render_children, Blocks};

pub(super) fn render_list(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, depth: usize) -> String {
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
