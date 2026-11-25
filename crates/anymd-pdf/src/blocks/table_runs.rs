//! Recover tables and side-by-side text from aligned runs of rows.

use std::ops::Range;

use crate::rows::{row_text, Segment};
use crate::tables::stream::{stream_table, Stream};

use super::{bullet_body, is_equation_number, join_line, Block, PageTables};

/// How far above a table's first aligned row its header starts: short
/// lines just above it (the top lines of stacked column headings) that sit
/// tight above the next line, within the table's width, and are still in the
/// open paragraph.
fn header_lines_above(
    rows: &[Vec<Segment>],
    index: usize,
    end: usize,
    marks: &[(usize, usize)],
) -> usize {
    let lo = rows[index..end]
        .iter()
        .flatten()
        .map(|s| s.x0)
        .fold(f64::INFINITY, f64::min);
    let hi = rows[index..end]
        .iter()
        .flatten()
        .map(|s| s.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut start = index;
    for &(row_index, _) in marks.iter().rev() {
        if row_index + 1 != start || index - row_index > 3 {
            break;
        }
        let row = &rows[row_index];
        let below = &rows[start];
        let size = row.iter().map(|s| s.size).fold(0.0, f64::max);
        let bottom = row.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min);
        let top = below
            .iter()
            .map(|s| s.top)
            .fold(f64::NEG_INFINITY, f64::max);
        let short = row.iter().all(|s| s.chars() <= 60);
        let inside = row.iter().all(|s| s.x0 >= lo - size && s.x1 <= hi + size);
        let text = row_text(row);
        if !(short && inside && bottom - top <= size * 0.8) || bullet_body(&text).is_some() {
            break;
        }
        start = row_index;
    }
    // Only a paragraph made of nothing but these lines: a short last line of
    // running text (a caption's tail) is not a header.
    match marks.first() {
        Some(&(first, _)) if first == start => start,
        _ => index,
    }
}

/// Blocks for side-by-side columns of running text: each column's lines
/// joined into paragraphs, left column first.
pub(super) fn column_blocks(columns: Vec<Vec<String>>, blocks: &mut Vec<Block>) {
    for column in columns {
        let mut paragraph = String::new();
        let mut lines = 0;
        for line in column.into_iter().chain([String::new()]) {
            if line.is_empty() {
                if !paragraph.is_empty() {
                    blocks.push(Block::Paragraph {
                        text: std::mem::take(&mut paragraph),
                        size: 0.0,
                        lines,
                    });
                }
                lines = 0;
                continue;
            }
            join_line(&mut paragraph, &line);
            lines += 1;
        }
    }
}

pub(super) fn stream_run(
    rows: &[Vec<Segment>],
    index: usize,
    marks: &[(usize, usize)],
    tables: &PageTables,
) -> Option<(Range<usize>, Stream)> {
    let is_multi = |row: &Vec<Segment>| {
        row.len() >= 2
            && !(row.len() == 2 && is_equation_number(&row[1].text))
            && row.iter().all(|s| s.table.is_none())
    };

    if !is_multi(&rows[index]) {
        return None;
    }
    let mut end = index;
    let mut multi = 0;
    // Column anchors (cell starts and ends) seen so far in this run.
    let mut anchors: Vec<f64> = Vec::new();
    let aligned = |row: &Vec<Segment>, anchors: &[f64]| {
        if anchors.is_empty() {
            return true;
        }
        let tolerance = row[0].size * 0.8;
        let hits = row
            .iter()
            .filter(|s| {
                anchors
                    .iter()
                    .any(|a| (a - s.x0).abs() <= tolerance || (a - s.x1).abs() <= tolerance)
            })
            .count();
        hits * 2 >= row.len()
    };
    while end < rows.len() {
        if is_multi(&rows[end]) {
            if multi >= 2 && !aligned(&rows[end], &anchors) {
                // A different grid starts here (e.g. a second author block).
                break;
            }
            anchors.extend(rows[end].iter().flat_map(|s| [s.x0, s.x1]));
            multi += 1;
            end += 1;
        } else if multi > 0 {
            // Short lines between aligned rows (wrapped labels) stay
            // in the table when an aligned row follows within a few
            // lines, or when they sit tight under the last one at
            // the table's left edge.
            let left = rows[index..end]
                .iter()
                .map(|r| r[0].x0)
                .fold(f64::INFINITY, f64::min);
            let bottom_of =
                |row: &Vec<Segment>| row.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min);
            // A wrapped label: one short line at the table's left
            // edge, tight under the line above it.
            // (Or tight above the next aligned row, when rows are
            // set apart by blank lines.)
            let label_line = |r: usize| {
                let row = &rows[r];
                let size = row[0].size;
                let next = rows.get(r + 1).filter(|next| is_multi(next));
                let tight_below =
                    next.is_some_and(|next| bottom_of(row) - next[0].top <= size * 0.45);
                // A header may leave the label column empty. The
                // first wrapped label then aligns with the next
                // value row, not with the header's first cell.
                let at_left = (row[0].x0 - left).abs() <= size * 1.5
                    || tables.ocr
                        && tight_below
                        && next.is_some_and(|next| (row[0].x0 - next[0].x0).abs() <= size * 1.5);
                row.len() == 1
                    && row[0].table.is_none()
                    && row[0].chars() <= 40
                    && at_left
                    && (bottom_of(&rows[r - 1]) - row[0].top <= size * 0.45 || tight_below)
            };
            let ahead = (end..rows.len().min(end + 4))
                .take_while(|&r| !is_multi(&rows[r]) && label_line(r))
                .count();
            if ahead > 0 && end + ahead < rows.len() && is_multi(&rows[end + ahead]) {
                end += ahead;
                continue;
            }
            if ahead > 0 {
                end += ahead;
            }
            break;
        } else {
            break;
        }
    }
    if multi < 2 {
        return None;
    }
    let start = header_lines_above(rows, index, end, marks);
    let found = stream_table(
        &rows[start..end],
        &tables.rules,
        tables.word_space,
        tables.ocr,
    );
    (found != Stream::Nothing).then_some((start..end, found))
}
