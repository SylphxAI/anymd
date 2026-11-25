//! Detect stacked headers and turn aligned lines into a grid with spans.

use super::alignment::AlignedRows;
use super::continuations::merge_continuations;
use super::Line;
use crate::extract::Rule;
use crate::rows::Segment;
use crate::tables::{first_data_row, Cell, Grid};

fn header_rows(lines: &[Line], size: f64) -> usize {
    let texts: Vec<Vec<String>> = lines.iter().map(|l| l.cells.clone()).collect();
    let data = first_data_row(&texts);
    // Header rows: the first row, plus following rows that are tight below it
    // and fill columns other than the first, up to the first row of numbers.
    let mut header_rows = 1;
    let header_limit = data.unwrap_or(if lines[0].spans.is_empty() { 1 } else { 2 });
    while header_rows < header_limit.min(8) && header_rows + 1 < lines.len() {
        let (upper, lower) = (&lines[header_rows - 1], &lines[header_rows]);
        let tight = upper.bottom - lower.top <= size * 0.8;
        let beyond_first = lower.cells.iter().skip(1).any(|c| !c.is_empty());
        if !(tight && beyond_first) {
            break;
        }
        header_rows += 1;
    }
    header_rows
}

pub(super) fn build_grid(
    rows: &[Vec<Segment>],
    rules: &[Rule],
    aligned: AlignedRows,
    ocr: bool,
) -> Option<Grid> {
    let header_rows = header_rows(&aligned.lines, aligned.size);
    let merged = merge_continuations(rows, rules, aligned, header_rows, ocr);
    if merged.len() < 2 {
        return None;
    }
    let cells = merged
        .iter()
        .enumerate()
        .map(|(r, line)| {
            let mut row: Vec<Option<Cell>> = line
                .cells
                .iter()
                .map(|text| {
                    Some(Cell {
                        text: text.clone(),
                        cols: 1,
                        rows: 1,
                    })
                })
                .collect();
            if r < header_rows {
                // A header phrase over several columns covers each of them.
                for &(first, last, placed) in &line.spans {
                    let text = row[placed]
                        .as_ref()
                        .map(|c| c.text.clone())
                        .unwrap_or_default();
                    if (first..=last).all(|c| c == placed || line.cells[c].is_empty()) {
                        row[placed] = Some(Cell::default());
                        row[first] = Some(Cell {
                            text,
                            cols: last - first + 1,
                            rows: 1,
                        });
                        for slot in row.iter_mut().take(last + 1).skip(first + 1) {
                            *slot = None;
                        }
                    }
                }
            }
            row
        })
        .collect();
    Some(Grid { cells, header_rows })
}
