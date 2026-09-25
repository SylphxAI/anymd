//! Tables drawn with ruling lines.
//!
//! The approach of pdfplumber's and Camelot's "lattice" mode: straight lines
//! are merged (dashed and per-cell pieces join up), lines that touch form a
//! component, and a component with at least two rows and two columns is a
//! grid. A missing line between two neighbouring grid cells merges them into
//! one spanning cell. Glyphs are placed in cells by their centre, and each
//! cell's text is laid out on its own, so a cell may hold several lines.

use crate::blocks::join_line;
use crate::extract::{Glyph, Rule};
use crate::rows::{row_text, rows_of, segments_of_row};
use crate::tables::{strip_leaders, Cell, Grid};

/// Distance within which two lines count as touching or as one line.
const SNAP: f64 = 2.0;
/// Pages with more line pairs than this are charts or hatching, not tables.
const MAX_PAIRS: usize = 4_000_000;

/// A grid found from ruling lines, with its box (x0, bottom, x1, top).
#[derive(Debug, Clone)]
pub(crate) struct RuledTable {
    pub(crate) x0: f64,
    pub(crate) bottom: f64,
    pub(crate) x1: f64,
    pub(crate) top: f64,
    pub(crate) content: Ruled,
}

/// What a ruled grid holds.
#[derive(Debug, Clone)]
pub(crate) enum Ruled {
    /// A table, with an optional caption row that spanned the whole grid.
    Table { caption: Option<String>, grid: Grid },
    /// Boxes around blocks of content (a layout frame, form sections, or
    /// tables drawn inside boxes): each box's glyphs, in reading order, to
    /// be laid out on their own.
    Frame(Vec<Vec<Glyph>>),
}

#[derive(Debug, Clone, Copy)]
struct Line {
    at: f64,
    from: f64,
    to: f64,
    /// Every piece is an edge of a shaded box.
    soft: bool,
}

/// Merge rules of one orientation: the same position within [`SNAP`], and
/// pieces that overlap or nearly touch along it.
fn merge(mut rules: Vec<Line>) -> Vec<Line> {
    rules.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.from.total_cmp(&b.from)));
    let mut clusters: Vec<Vec<Line>> = Vec::new();
    for rule in rules {
        match clusters.last_mut() {
            Some(cluster) if (rule.at - cluster[0].at).abs() <= SNAP / 2.0 => cluster.push(rule),
            _ => clusters.push(vec![rule]),
        }
    }
    let mut out = Vec::new();
    for mut cluster in clusters {
        let at = cluster.iter().map(|l| l.at).sum::<f64>() / cluster.len() as f64;
        cluster.sort_by(|a, b| a.from.total_cmp(&b.from));
        let mut current: Option<Line> = None;
        for line in cluster {
            match current.as_mut() {
                Some(run) if line.from <= run.to + SNAP => {
                    run.to = run.to.max(line.to);
                    run.soft &= line.soft;
                }
                _ => {
                    out.extend(current.take());
                    current = Some(Line { at, ..line });
                }
            }
        }
        out.extend(current);
    }
    out.retain(|line| line.to - line.from >= SNAP * 2.0);
    out
}

fn find(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra] = rb;
    }
}

/// Positions within [`SNAP`] of each other become one (their mean).
fn cluster_positions(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    let mut out: Vec<Vec<f64>> = Vec::new();
    for value in values {
        match out.last_mut() {
            Some(group) if value - group[group.len() - 1] <= SNAP => group.push(value),
            _ => out.push(vec![value]),
        }
    }
    out.into_iter()
        .map(|group| group.iter().sum::<f64>() / group.len() as f64)
        .collect()
}

/// Whether some line at `at` covers most of `from..to`.
fn covered(lines: &[&Line], at: f64, from: f64, to: f64) -> bool {
    let need = (to - from) * 0.6;
    lines.iter().any(|line| {
        (line.at - at).abs() <= SNAP && line.to.min(to) - line.from.max(from) >= need
    })
}

/// Find ruled tables. Returns the tables and the glyphs that are not inside
/// any of them.
pub(crate) fn ruled_tables(rules: &[Rule], glyphs: Vec<Glyph>) -> (Vec<RuledTable>, Vec<Glyph>) {
    let horizontal = merge(
        rules
            .iter()
            .filter(|r| r.horizontal)
            .map(|r| Line { at: r.at, from: r.from, to: r.to, soft: r.soft })
            .collect(),
    );
    let vertical = merge(
        rules
            .iter()
            .filter(|r| !r.horizontal)
            .map(|r| Line { at: r.at, from: r.from, to: r.to, soft: r.soft })
            .collect(),
    );
    if horizontal.len() < 2
        || vertical.is_empty()
        || horizontal.len().saturating_mul(vertical.len()) > MAX_PAIRS
    {
        return (Vec::new(), glyphs);
    }
    // Components of touching lines: horizontals first, then verticals.
    let count = horizontal.len() + vertical.len();
    let mut parent: Vec<usize> = (0..count).collect();
    for (h, hl) in horizontal.iter().enumerate() {
        for (v, vl) in vertical.iter().enumerate() {
            if vl.at >= hl.from - SNAP
                && vl.at <= hl.to + SNAP
                && hl.at >= vl.from - SNAP
                && hl.at <= vl.to + SNAP
            {
                union(&mut parent, h, horizontal.len() + v);
            }
        }
    }
    let mut components: std::collections::BTreeMap<usize, (Vec<&Line>, Vec<&Line>)> =
        std::collections::BTreeMap::new();
    for index in 0..count {
        let root = find(&mut parent, index);
        let entry = components.entry(root).or_default();
        if index < horizontal.len() {
            entry.0.push(&horizontal[index]);
        } else {
            entry.1.push(&vertical[index - horizontal.len()]);
        }
    }

    let mut tables = Vec::new();
    let mut claimed: Vec<bool> = vec![false; glyphs.len()];
    for (hs, vs) in components.into_values() {
        if hs.len() < 2 || vs.is_empty() {
            continue;
        }
        let Some(table) = grid_of(&hs, &vs, &glyphs, &mut claimed) else {
            continue;
        };
        tables.push(table);
    }
    let rest = glyphs
        .into_iter()
        .zip(claimed)
        .filter_map(|(glyph, taken)| (!taken).then_some(glyph))
        .collect();
    (tables, rest)
}

fn grid_of(hs: &[&Line], vs: &[&Line], glyphs: &[Glyph], claimed: &mut [bool]) -> Option<RuledTable> {
    // Column edges: the verticals, plus the ends of the horizontals for
    // tables without outer side lines. Row edges likewise.
    let mut xs: Vec<f64> = vs.iter().map(|v| v.at).collect();
    xs.push(hs.iter().map(|h| h.from).fold(f64::INFINITY, f64::min));
    xs.push(hs.iter().map(|h| h.to).fold(f64::NEG_INFINITY, f64::max));
    let xs = cluster_positions(xs);
    let mut ys: Vec<f64> = hs.iter().map(|h| h.at).collect();
    ys.push(vs.iter().map(|v| v.from).fold(f64::INFINITY, f64::min));
    ys.push(vs.iter().map(|v| v.to).fold(f64::NEG_INFINITY, f64::max));
    let mut ys = cluster_positions(ys);
    ys.reverse(); // top to bottom
    let (ncols, nrows) = (xs.len().checked_sub(1)?, ys.len().checked_sub(1)?);
    if ncols < 2 || nrows < 2 || ncols * nrows > 10_000 {
        return None;
    }
    let (x0, x1, top, bottom) = (xs[0], xs[ncols], ys[0], ys[nrows]);
    // Merge neighbouring grid cells that no line separates.
    let id = |r: usize, c: usize| r * ncols + c;
    let mut parent: Vec<usize> = (0..nrows * ncols).collect();
    for r in 0..nrows {
        for c in 0..ncols {
            if c + 1 < ncols && !covered(vs, xs[c + 1], ys[r + 1], ys[r]) {
                union(&mut parent, id(r, c), id(r, c + 1));
            }
            if r + 1 < nrows && !covered(hs, ys[r + 1], xs[c], xs[c + 1]) {
                union(&mut parent, id(r, c), id(r + 1, c));
            }
        }
    }
    // Each merged cell's extent in grid rows and columns.
    let mut extent: std::collections::HashMap<usize, (usize, usize, usize, usize)> =
        std::collections::HashMap::new();
    for r in 0..nrows {
        for c in 0..ncols {
            let root = find(&mut parent, id(r, c));
            let e = extent.entry(root).or_insert((r, c, r, c));
            e.0 = e.0.min(r);
            e.1 = e.1.min(c);
            e.2 = e.2.max(r);
            e.3 = e.3.max(c);
        }
    }
    // Place glyphs by their centre.
    let mut members: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for (index, glyph) in glyphs.iter().enumerate() {
        if claimed[index] || glyph.space {
            continue;
        }
        let cx = (glyph.x0 + glyph.x1) / 2.0;
        let cy = glyph.base + glyph.size * 0.3;
        if cx <= x0 || cx >= x1 || cy >= top || cy <= bottom {
            continue;
        }
        let c = xs.partition_point(|&x| x < cx).saturating_sub(1).min(ncols - 1);
        let r = ys.partition_point(|&y| y > cy).saturating_sub(1).min(nrows - 1);
        members.entry(find(&mut parent, id(r, c))).or_default().push(index);
    }
    // Each cell's lines: (baseline, text).
    let mut lines: std::collections::HashMap<usize, Vec<(f64, String)>> =
        std::collections::HashMap::new();
    for (root, indexes) in &members {
        lines.insert(*root, cell_lines(indexes.iter().map(|&i| glyphs[i].clone()).collect()));
    }
    let (cells, filled, chars, longest) = build_cells(nrows, ncols, &extent, &lines);
    let mut grid = Grid { cells, header_rows: 0 };
    drop_empty(&mut grid);
    let (rows, cols) = (grid.cells.len(), grid.width());
    // A grid is a table when it has real rows and columns of short text, not
    // a chart's gridlines (mostly empty). Boxes around prose or around
    // tables of their own are a frame: each box is read on its own.
    if rows < 2 || cols < 2 || filled < 3 {
        return None;
    }
    let sparse = (filled as f64) < (rows * cols) as f64 * 0.25;
    // Boxes that hold a table of their own (most lines split into cells) or
    // a passage of prose. Two or more make the grid a frame around separate
    // content. One such box beside plain cells is a table with a single
    // drawn line (a highlighted last column): the whitespace finder reads it
    // whole.
    let _ = (chars, longest);
    let content_columns: std::collections::BTreeSet<usize> = members
        .iter()
        .filter(|(_, indexes)| {
            let glyphs: Vec<Glyph> = indexes.iter().map(|&i| glyphs[i].clone()).collect();
            let text_len = glyphs.iter().filter(|g| !g.space).count();
            let letters = glyphs
                .iter()
                .filter(|g| g.text.chars().any(char::is_alphabetic))
                .count();
            let rows = rows_of(glyphs);
            let lines = rows.len();
            let split = rows
                .into_iter()
                .filter(|row| segments_of_row(row.clone()).len() >= 2)
                .count();
            (split >= 3 && split * 2 >= lines) || (text_len > 300 && letters * 10 >= text_len * 7)
        })
        .filter_map(|(root, _)| extent.get(root).map(|e| e.1))
        .collect();
    let content_boxes = content_columns.len();
    let framed = content_boxes >= 2 && vs.iter().all(|v| !v.soft);
    if (sparse || content_boxes > 0) && !framed {
        return None;
    }
    // Claim the glyphs inside the grid.
    for (index, glyph) in glyphs.iter().enumerate() {
        let cx = (glyph.x0 + glyph.x1) / 2.0;
        let cy = glyph.base + glyph.size * 0.3;
        if cx > x0 && cx < x1 && cy < top && cy > bottom {
            claimed[index] = true;
        }
    }
    let content = if framed {
        let mut boxes: Vec<(&(usize, usize, usize, usize), &Vec<usize>)> = extent
            .iter()
            .filter_map(|(root, e)| members.get(root).map(|m| (e, m)))
            .collect();
        boxes.sort_by_key(|(e, _)| (e.0, e.1));
        Ruled::Frame(
            boxes
                .into_iter()
                .map(|(_, m)| m.iter().map(|&i| glyphs[i].clone()).collect())
                .collect(),
        )
    } else {
        let caption = take_caption(&mut grid);
        grid.header_rows = ruled_header_rows(&grid);
        Ruled::Table { caption, grid }
    };
    if std::env::var("ANYMD_DEBUG_RULED").is_ok() { eprintln!("RULED bbox=({x0:.0},{bottom:.0},{x1:.0},{top:.0}) grid={}x{} kind={}", rows, cols, match &content { Ruled::Table{grid,..} => format!("table h={} first={:?}", grid.header_rows, grid.cells.first()), Ruled::Frame(b) => format!("frame {}", b.len()) }); }
    Some(RuledTable { x0, bottom, x1, top, content })
}

/// The lines of one cell, top to bottom, with their baselines.
fn cell_lines(glyphs: Vec<Glyph>) -> Vec<(f64, String)> {
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
        join_line(&mut text, line);
    }
    strip_leaders(&text)
}

type Extent = (usize, usize, usize, usize);

/// The grid's cells. A grid row whose cells all hold the same number of
/// lines on shared baselines, mostly numbers, is several table rows drawn
/// between two rules (banded tables): it is split into one row per line.
/// Returns the cells, the number of cells with text, their total characters
/// and the longest cell.
fn build_cells(
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
        let k = texts.first().map_or(0, |l| l.len());
        let numbers = texts
            .iter()
            .flat_map(|l| l.iter())
            .filter(|(_, t)| crate::tables::is_numeric(t))
            .count();
        let total: usize = texts.iter().map(|l| l.len()).sum();
        let aligned = texts.iter().all(|l| {
            l.len() == k && l.iter().zip(texts[0].iter()).all(|(a, b)| (a.0 - b.0).abs() <= 2.0)
        });
        let split = k >= 2
            && texts.len() >= 2
            && aligned
            && numbers * 2 >= total
            && !crossing[r]
            && starts.iter().all(|(_, e)| e.2 == e.0);
        let height = if split { k } else { 1 };
        let base = cells.len();
        cells.extend((0..height).map(|_| vec![None; ncols]));
        for (root, e) in starts {
            let cell_lines = lines.get(root).unwrap_or(&empty);
            for part in 0..height {
                let text = if split {
                    cell_lines.get(part).map(|(_, t)| strip_leaders(t)).unwrap_or_default()
                } else {
                    joined(cell_lines)
                };
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
fn drop_empty(grid: &mut Grid) {
    let mut r = 0;
    while r < grid.cells.len() {
        if grid.cells[r].iter().flatten().any(|cell| !cell.text.is_empty()) {
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
fn take_caption(grid: &mut Grid) -> Option<String> {
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
fn ruled_header_rows(grid: &Grid) -> usize {
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
