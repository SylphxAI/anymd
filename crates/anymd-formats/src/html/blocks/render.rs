//! Selects block and inline renderers while walking the HTML tree.

use ego_tree::NodeRef;
use scraper::Node;

use super::super::extract::{element_name, is_block, is_dropped, BLOCK_TAGS};
use super::super::inline::{inline_string, plain_text, push_text, render_inline};
use super::super::{Ctx, MAX_DEPTH};
use super::list::render_list;
use super::pre::render_pre;
use super::table::render_table;
use super::{finish_blocks, Blocks};

pub(super) fn render_children(
    node: NodeRef<'_, Node>,
    ctx: &Ctx<'_>,
    out: &mut Blocks,
    depth: usize,
) {
    for child in node.children() {
        render_block(child, ctx, out, depth + 1);
    }
}

pub(in crate::html) fn render_block(
    node: NodeRef<'_, Node>,
    ctx: &Ctx<'_>,
    out: &mut Blocks,
    depth: usize,
) {
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
