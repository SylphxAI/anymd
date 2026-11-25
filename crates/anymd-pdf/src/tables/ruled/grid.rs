//! Build and classify a grid from one connected set of ruling lines.

use crate::extract::Glyph;
use crate::rows::{rows_of, segments_of_row};
use crate::tables::Grid;

use super::cells::{build_cells, cell_lines, drop_empty, ruled_header_rows, take_caption};
use super::geometry::{cluster_positions, covered, find, union, Line, SNAP};
use super::{Ruled, RuledTable};

pub(super) fn grid_of(
    hs: &[&Line],
    vs: &[&Line],
    glyphs: &[Glyph],
    claimed: &mut [bool],
) -> Option<RuledTable> {
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
    let mut members: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for (index, glyph) in glyphs.iter().enumerate() {
        if claimed[index] || glyph.space {
            continue;
        }
        let cx = (glyph.x0 + glyph.x1) / 2.0;
        let cy = glyph.base + glyph.size * 0.3;
        if cx <= x0 || cx >= x1 || cy >= top || cy <= bottom {
            continue;
        }
        let c = xs
            .partition_point(|&x| x < cx)
            .saturating_sub(1)
            .min(ncols - 1);
        let r = ys
            .partition_point(|&y| y > cy)
            .saturating_sub(1)
            .min(nrows - 1);
        members
            .entry(find(&mut parent, id(r, c)))
            .or_default()
            .push(index);
    }
    // Each cell's lines: (baseline, text).
    let mut lines: std::collections::HashMap<usize, Vec<(f64, String)>> =
        std::collections::HashMap::new();
    for (root, indexes) in &members {
        lines.insert(
            *root,
            cell_lines(indexes.iter().map(|&i| glyphs[i].clone()).collect()),
        );
    }
    let (cells, filled, chars, longest) = build_cells(nrows, ncols, &extent, &lines);
    // Text never runs across a table's column line; it does across a
    // chart's gridlines (a legend, an axis title).
    if crosses_lines(&xs, &ys, vs, glyphs, claimed, (x0, bottom, x1, top)) {
        return None;
    }
    let mut grid = Grid {
        cells,
        header_rows: 0,
    };
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
        type ContentBox<'a> = (&'a (usize, usize, usize, usize), &'a Vec<usize>);
        let mut boxes: Vec<ContentBox<'_>> = extent
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
    Some(RuledTable {
        x0,
        bottom,
        x1,
        top,
        content,
    })
}

/// Whether words run across the grid's inner vertical lines: two glyphs of
/// one word (closer than a word space) on either side of a drawn line.
fn crosses_lines(
    xs: &[f64],
    ys: &[f64],
    vs: &[&Line],
    glyphs: &[Glyph],
    claimed: &[bool],
    (x0, bottom, x1, top): (f64, f64, f64, f64),
) -> bool {
    let inside: Vec<Glyph> = glyphs
        .iter()
        .zip(claimed)
        .filter(|(g, taken)| {
            let cx = (g.x0 + g.x1) / 2.0;
            let cy = g.base + g.size * 0.3;
            !**taken && !g.space && cx > x0 && cx < x1 && cy < top && cy > bottom
        })
        .map(|(g, _)| g.clone())
        .collect();
    let inner = &xs[1..xs.len() - 1];
    let mut crossings = 0;
    for mut row in rows_of(inside) {
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        for pair in row.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if b.x0 - a.x1 > a.size.max(b.size) * 0.15 {
                continue;
            }
            let y = a.base + a.size * 0.3;
            let drawn = |x: f64| {
                vs.iter()
                    .any(|v| (v.at - x).abs() <= SNAP && v.from <= y && v.to >= y)
            };
            if inner
                .iter()
                .any(|&x| (a.x0 + a.x1) / 2.0 < x && (b.x0 + b.x1) / 2.0 > x && drawn(x))
            {
                crossings += 1;
            }
        }
    }
    let _ = ys;
    crossings >= 2
}
