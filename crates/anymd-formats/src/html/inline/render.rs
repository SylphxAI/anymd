//! Inline DOM traversal and tag dispatch.

use ego_tree::NodeRef;
use scraper::Node;

use super::super::extract::{is_dropped, BLOCK_TAGS};
use super::super::{Ctx, MAX_DEPTH};
use super::links::{render_image, render_link};
use super::markdown::{clean_tex, longest_run, push_atom, push_text, trim_trailing_space, wrap};
use super::text::{collapse, plain_text};

fn render_inline_children(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, buf: &mut String, depth: usize) {
    for child in node.children() {
        if ctx.failure.borrow().is_some() {
            break;
        }
        render_inline(child, ctx, buf, depth + 1);
        if let Err(error) = crate::limits::check_output(buf) {
            *ctx.failure.borrow_mut() = Some(error);
            break;
        }
    }
}

pub(in crate::html) fn inline_string(
    node: NodeRef<'_, Node>,
    ctx: &Ctx<'_>,
    depth: usize,
) -> String {
    // The sentinel keeps a leading space significant ("a<b> b</b>" → "a **b**").
    let mut buf = String::from("\u{1}");
    render_inline_children(node, ctx, &mut buf, depth);
    buf.split_off(1)
}

pub(in crate::html) fn render_inline(
    node: NodeRef<'_, Node>,
    ctx: &Ctx<'_>,
    buf: &mut String,
    depth: usize,
) {
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
        "a" => render_link(node, ctx, buf, &inline_string(node, ctx, depth)),
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
