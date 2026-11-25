//! Pipe tables and the fallback for layout tables.

use ego_tree::NodeRef;
use scraper::Node;

use super::super::extract::{element_name, is_dropped};
use super::super::inline::{collapse, inline_string};
use super::{finish_blocks, render_children, Blocks, Ctx};

pub(super) fn render_table(node: NodeRef<'_, Node>, ctx: &Ctx<'_>, out: &mut Blocks, depth: usize) {
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
                cells.push(cell_text(&finish_blocks(inner, "\n", ctx)));
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
    match crate::bounded_markdown_table(&rows) {
        Ok(table) => out.push(table.trim_end().to_string()),
        Err(error) => *ctx.failure.borrow_mut() = Some(error),
    }
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
