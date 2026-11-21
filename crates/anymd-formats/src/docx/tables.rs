//! Table grids, nested cells, and Markdown table rendering.

use super::review::{change_of, revision_marks};
use super::Writer;
use crate::bounded_markdown_table;
use crate::critic::{self, Change};
use crate::ooxml::Element;

impl Writer<'_> {
    pub(super) fn table(&mut self, table: &Element) -> String {
        let mut rows = Vec::new();
        for tr in table_rows(table) {
            let mut row = Vec::new();
            let row_marks = tr.child("trPr").map(revision_marks).unwrap_or_default();
            let before = tr
                .path(&["trPr", "gridBefore"])
                .and_then(|g| g.attr("val"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            row.extend(std::iter::repeat_n(String::new(), before.min(64)));
            for tc in row_cells(tr) {
                row.push(self.cell_text(&row_marks, tc));
                let span = tc
                    .path(&["tcPr", "gridSpan"])
                    .and_then(|g| g.attr("val"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1);
                row.extend(std::iter::repeat_n(String::new(), span.clamp(1, 64) - 1));
            }
            rows.push(row);
        }
        // Drop trailing empty columns and fully empty rows.
        rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
        if rows.is_empty() {
            return String::new();
        }
        let width = (0..rows.iter().map(Vec::len).max().unwrap_or(0))
            .rev()
            .find(|&c| {
                rows.iter()
                    .any(|r| r.get(c).is_some_and(|v| !v.trim().is_empty()))
            })
            .map_or(0, |c| c + 1);
        for row in &mut rows {
            row.truncate(width);
        }
        bounded_markdown_table(&rows).unwrap_or_else(|error| {
            self.failure = Some(error);
            String::new()
        })
    }

    /// A cell's paragraphs joined with `<br>`, inside the tracked changes of its
    /// row (`trPr`) and of the cell itself (`tcPr`).
    fn cell_text(&mut self, row_marks: &[Change], cell: &Element) -> String {
        let depth = self.revisions.len();
        self.revisions.extend_from_slice(row_marks);
        if let Some(properties) = cell.child("tcPr") {
            self.revisions.extend(revision_marks(properties));
        }
        let mut parts = Vec::new();
        self.cell_parts(cell, &mut parts);
        self.revisions.truncate(depth);
        critic::join_marked(parts, "<br>").replace('\n', "<br>")
    }

    /// Paragraph texts in a cell, each with the tracked change on its mark.
    fn cell_parts(&mut self, container: &Element, parts: &mut Vec<critic::Part>) {
        for child in container.elements() {
            match child.local() {
                "p" => {
                    let (inline, extra) = self.paragraph_inline(child);
                    let mark = self.paragraph_mark(child).filter(|_| extra.is_empty());
                    let inline = inline.into_inline();
                    parts.push((inline.render(true), mark, inline.edges()));
                    parts.extend(extra.into_iter().map(|block| (block, None, (false, false))));
                }
                "tbl" => {
                    // Nested tables are flattened to text, one row per line.
                    self.flattening += 1;
                    for tr in table_rows(child) {
                        let row_marks = tr.child("trPr").map(revision_marks).unwrap_or_default();
                        let cells: Vec<String> = row_cells(tr)
                            .map(|tc| self.cell_text(&row_marks, tc))
                            .filter(|t| !t.is_empty())
                            .collect();
                        parts.push((critic::join_changed(cells, "; "), None, (false, false)));
                    }
                    self.flattening -= 1;
                }
                "ins" | "moveTo" | "del" | "moveFrom" => {
                    self.revised(change_of(child), |w| w.cell_parts(child, parts));
                }
                "commentRangeStart" | "commentRangeEnd" => self.comment_range(child, None),
                "sdt" | "sdtContent" | "customXml" => self.cell_parts(child, parts),
                _ => {}
            }
        }
    }
}

fn table_rows(table: &Element) -> Vec<&Element> {
    let mut rows = Vec::new();
    collect_named(
        table,
        "tr",
        &["sdt", "sdtContent", "customXml", "ins"],
        &mut rows,
    );
    rows
}

fn row_cells(row: &Element) -> impl Iterator<Item = &Element> {
    let mut cells = Vec::new();
    collect_named(
        row,
        "tc",
        &["sdt", "sdtContent", "customXml", "ins"],
        &mut cells,
    );
    cells.into_iter()
}

fn collect_named<'a>(
    container: &'a Element,
    name: &str,
    wrappers: &[&str],
    out: &mut Vec<&'a Element>,
) {
    for child in container.elements() {
        if child.is(name) {
            out.push(child);
        } else if wrappers.contains(&child.local()) {
            collect_named(child, name, wrappers, out);
        }
    }
}
