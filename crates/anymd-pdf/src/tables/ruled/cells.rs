//! Assemble cell text and clean up the rows and columns of a ruled grid.

use crate::extract::Glyph;
use crate::rows::{row_text, rows_of, segments_of_row};
use crate::tables::{join_cell_line, strip_leaders, Cell, Grid};

/// The lines of one cell, top to bottom, with their baselines.
pub(super) fn cell_lines(glyphs: Vec<Glyph>) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for row in rows_of(glyphs) {
        let segments = segments_of_row(row);
        let line = row_text(&segments);
        if let Some(first) = segments.first() {
            if !line.is_empty() {
                out.push((first.base, line));
            }
        }
    }
    out
}

/// Lines joined like a paragraph, without leader dots.
fn joined(lines: &[(f64, String)]) -> String {
    let mut text = String::new();
    for (_, line) in lines {
        join_cell_line(&mut text, line);
    }
    strip_leaders(&text)
}

type Extent = (usize, usize, usize, usize);

/// The grid's cells. A grid row whose cells all hold the same number of
/// lines on shared baselines, mostly numbers, is several table rows drawn
/// between two rules (banded tables): it is split into one row per line.
/// Returns the cells, the number of cells with text, their total characters
/// and the longest cell.
pub(super) fn build_cells(
    nrows: usize,
    ncols: usize,
    extent: &std::collections::HashMap<usize, Extent>,
    lines: &std::collections::HashMap<usize, Vec<(f64, String)>>,
) -> (Vec<Vec<Option<Cell>>>, usize, usize, usize) {
    let empty = Vec::new();
    // Cells by the grid row they start in.
    let mut by_row: Vec<Vec<(usize, Extent)>> = vec![Vec::new(); nrows];
    for (root, e) in extent {
        by_row[e.0].push((*root, *e));
    }
    let mut crossing = vec![false; nrows];
    for e in extent.values() {
        for flag in crossing.iter_mut().take(e.2 + 1).skip(e.0 + 1) {
            *flag = true;
        }
    }
    let mut cells: Vec<Vec<Option<Cell>>> = Vec::new();
    let (mut filled, mut chars, mut longest) = (0, 0, 0);
    let mut count = |text: &str| {
        if !text.is_empty() {
            let n = text.chars().count();
            filled += 1;
            chars += n;
            longest = longest.max(n);
        }
    };
    for (r, starts) in by_row.iter().enumerate() {
        let texts: Vec<&Vec<(f64, String)>> = starts
            .iter()
            .map(|(root, _)| lines.get(root).unwrap_or(&empty))
            .filter(|l| !l.is_empty())
            .collect();
        // Baselines shared by the lines of the row's cells, top to bottom.
        let mut bases: Vec<f64> = texts
            .iter()
            .flat_map(|l| l.iter().map(|(b, _)| *b))
            .collect();
        bases.sort_by(|a, b| b.total_cmp(a));
        let mut clusters: Vec<f64> = Vec::new();
        for base in bases {
            if clusters.last().is_none_or(|last| last - base > 2.0) {
                clusters.push(base);
            }
        }
        let cluster_of = |base: f64| {
            clusters
                .iter()
                .enumerate()
                .min_by(|a, b| (a.1 - base).abs().total_cmp(&(b.1 - base).abs()))
                .map_or(0, |(i, _)| i)
        };
        let shared = clusters
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                texts
                    .iter()
                    .filter(|l| l.iter().any(|(b, _)| cluster_of(*b) == *i))
                    .count()
                    >= 2
            })
            .count();
        let numbers = texts
            .iter()
            .flat_map(|l| l.iter())
            .filter(|(_, t)| crate::tables::is_numeric(t))
            .count();
        let total: usize = texts.iter().map(|l| l.len()).sum();
        let split = shared >= 2
            && texts.len() >= 2
            && numbers * 2 >= total
            && !crossing[r]
            && starts.iter().all(|(_, e)| e.2 == e.0);
        let height = if split { clusters.len() } else { 1 };
        let base = cells.len();
        cells.extend((0..height).map(|_| vec![None; ncols]));
        for (root, e) in starts {
            let cell_lines = lines.get(root).unwrap_or(&empty);
            let mut parts = vec![String::new(); height];
            if split {
                for (line_base, text) in cell_lines {
                    join_cell_line(&mut parts[cluster_of(*line_base)], text);
                }
            } else {
                parts[0] = joined(cell_lines);
            }
            for (part, text) in parts.into_iter().enumerate() {
                let text = strip_leaders(&text);
                count(&text);
                cells[base + part][e.1] = Some(Cell {
                    text,
                    cols: e.3 - e.1 + 1,
                    rows: if split { 1 } else { e.2 - e.0 + 1 },
                });
            }
        }
    }
    (cells, filled, chars, longest)
}

/// Remove grid rows and columns in which no cell starts with text. Cells
/// that spanned a removed row or column become one shorter.
pub(super) fn drop_empty(grid: &mut Grid) {
    let mut r = 0;
    while r < grid.cells.len() {
        if grid.cells[r]
            .iter()
            .flatten()
            .any(|cell| !cell.text.is_empty())
        {
            r += 1;
            continue;
        }
        for (above, row) in grid.cells[..r].iter_mut().enumerate() {
            for cell in row.iter_mut().flatten() {
                if above + cell.rows > r {
                    cell.rows -= 1;
                }
            }
        }
        grid.cells.remove(r);
    }
    let mut c = 0;
    while c < grid.width() {
        let used = grid
            .cells
            .iter()
            .any(|row| row[c].as_ref().is_some_and(|cell| !cell.text.is_empty()));
        if used {
            c += 1;
            continue;
        }
        for row in grid.cells.iter_mut() {
            for (left, cell) in row.iter_mut().enumerate().take(c) {
                if let Some(cell) = cell {
                    if left + cell.cols > c {
                        cell.cols -= 1;
                    }
                }
            }
            row.remove(c);
        }
    }
}

/// A first row that is one cell across the whole grid is the table's title.
pub(super) fn take_caption(grid: &mut Grid) -> Option<String> {
    let width = grid.width();
    let first = grid.cells.first()?;
    match first.first()? {
        Some(cell) if cell.cols >= width && width >= 2 && grid.cells.len() > 2 => {
            let text = cell.text.clone();
            grid.cells.remove(0);
            Some(text)
        }
        _ => None,
    }
}

/// Header rows of a ruled grid: the first row, extended while a header cell
/// spans columns (a group heading over sub-headings) or reaches down into the
/// next row; and never past the first row of numbers.
pub(super) fn ruled_header_rows(grid: &Grid) -> usize {
    let rows = grid.cells.len();
    let width = grid.width();
    let mut header = 1;
    let mut r = 0;
    while r < header && r < rows {
        for cell in grid.cells[r].iter().flatten() {
            if cell.rows > 1 {
                header = header.max(r + cell.rows);
            }
            if cell.cols > 1 && cell.cols < width && !cell.text.is_empty() {
                header = header.max(r + 2);
            }
        }
        r += 1;
    }
    header.min(rows.saturating_sub(1)).clamp(1, 4)
}
