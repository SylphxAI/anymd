//! Merge wrapped cell text without merging distinct value rows.

use super::alignment::AlignedRows;
use super::{starts_lower, Line};
use crate::extract::Rule;
use crate::rows::Segment;
use crate::tables::{is_numeric, join_cell_line};

fn ends_open(text: &str) -> bool {
    const LINKS: &[&str] = &[
        "and", "or", "of", "the", "for", "to", "in", "with", "by", "on", "at", "from", "a", "an",
        "&",
    ];
    let text = text.trim_end();
    if text.ends_with('-') || text.ends_with(',') || text.ends_with('&') {
        return true;
    }
    let last = text.rsplit(' ').next().unwrap_or("").to_lowercase();
    LINKS.contains(&last.as_str())
}

pub(super) fn merge_continuations(
    rows: &[Vec<Segment>],
    rules: &[Rule],
    aligned: AlignedRows,
    header_rows: usize,
    ocr: bool,
) -> Vec<Line> {
    let AlignedRows { lines, width, size } = aligned;
    let x0 = rows
        .iter()
        .flatten()
        .map(|s| s.x0)
        .fold(f64::INFINITY, f64::min);
    let x1 = rows
        .iter()
        .flatten()
        .map(|s| s.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    let row_rules: Vec<f64> = rules
        .iter()
        .filter(|r| r.horizontal && r.to.min(x1) - r.from.max(x0) >= (x1 - x0) * 0.5)
        .map(|r| r.at)
        .collect();
    let ruled_between = |upper: &Line, lower: &Line| {
        row_rules
            .iter()
            .any(|&y| y < upper.bottom + 1.0 && y > lower.top - 1.0)
    };

    let separated_rows = lines
        .windows(2)
        .filter(|pair| ruled_between(&pair[0], &pair[1]))
        .count();

    let table_left = lines
        .iter()
        .filter_map(|line| {
            line.cells
                .iter()
                .position(|cell| !cell.is_empty())
                .filter(|&column| {
                    (0..width).any(|other| other != column && is_numeric(&line.cells[other]))
                })
        })
        .min()
        .unwrap_or(0);

    // Merge continuation lines into the row above.
    let mut merged: Vec<Line> = Vec::with_capacity(lines.len());
    for (index, line) in lines.into_iter().enumerate() {
        let Some(prev) = merged.last_mut().filter(|_| index > header_rows) else {
            merged.push(line);
            continue;
        };
        let tight = prev.bottom - line.top <= size * 0.45;
        if !tight || ruled_between(prev, &line) {
            merged.push(line);
            continue;
        }
        let filled: Vec<usize> = (0..width).filter(|&c| !line.cells[c].is_empty()).collect();
        let prev_filled: Vec<usize> = (0..width).filter(|&c| !prev.cells[c].is_empty()).collect();
        let any_number = filled.iter().any(|&c| is_numeric(&line.cells[c]));
        let prev_number = prev_filled.iter().any(|&c| is_numeric(&prev.cells[c]));
        let first = &line.cells[0];
        // The rest of a description whose first column is blank: it fills
        // fewer columns than the row above, or every cell reads on from the
        // one above. (A row that fills every column with values of its own
        // belongs to a label that spans rows.)
        let prev_values = prev_filled.iter().filter(|&&c| c > 0).count();
        let reads_on = filled
            .iter()
            .all(|&c| starts_lower(&line.cells[c]) || ends_open(&prev.cells[c]));
        let continues_cells = first.is_empty()
            && !any_number
            && !filled.is_empty()
            && filled.iter().all(|c| !prev.cells[*c].is_empty())
            && (filled.len() < prev_values || reads_on);
        // A label centred on a group of rows sits between the first two of
        // them: it belongs to the first.
        let centred_label = filled == [0]
            && prev.cells[0].is_empty()
            && prev_values > 0
            && (prev.top - line.top) < size * 0.8;
        // The first line of a wrapped label, with the values on the next line.
        let label_head = prev_filled == [0]
            && !prev_number
            && !first.is_empty()
            && (starts_lower(first) || ends_open(&prev.cells[0]));
        // A wrapped item label: the item's first line has its marker and the
        // start of the label, the next line has no marker, the rest of the
        // label and the values.
        let first_value = (0..width).find(|&c| is_numeric(&line.cells[c]));
        let item_head = !prev_filled
            .iter()
            .any(|&c| c > 0 && is_numeric(&prev.cells[c]))
            && !prev.cells[0].is_empty()
            && first.is_empty()
            && !prev_filled.iter().any(|&c| prev.cells[c].ends_with(':'))
            && first_value.is_some_and(|v| prev_filled.iter().all(|&c| c < v));
        // The rest of a wrapped label, below the line with the values.
        let label_tail = filled == [0]
            && !prev.cells[0].is_empty()
            && !any_number
            && (starts_lower(first) || ends_open(&prev.cells[0]));
        // A label set on its own line, with the rest of the label and the
        // values on the line below: "Advanced Vehicle" over "Development
        // 70.620 ...". It starts where the table's labels start. A heading
        // that closes with a stop, and a line of prose above the table, do
        // not.
        let prev_left = (0..width).find(|&column| !prev.cells[column].is_empty());
        let label_continues = ocr
            && prev_filled == [0]
            && !prev_number
            && !first.is_empty()
            && any_number
            && prev_left == Some(table_left)
            && !prev.cells[0].ends_with(['.', ':', ';', '?', '!']);
        // Rows separated by rules: text between two rules that reads on is
        // one row (a group of value rows between rules is not).
        let same_band = separated_rows >= 2
            && !any_number
            && (filled.iter().filter(|&&c| c > 0).count() < prev_values || reads_on);
        if continues_cells
            || centred_label
            || label_head
            || item_head
            || label_tail
            || label_continues
            || same_band
        {
            for (c, text) in line.cells.iter().enumerate() {
                join_cell_line(&mut prev.cells[c], text);
            }
            prev.bottom = prev.bottom.min(line.bottom);
        } else {
            merged.push(line);
        }
    }
    merged
}
