//! Tables drawn with ruling lines.
//!
//! The approach of pdfplumber's and Camelot's "lattice" mode: straight lines
//! are merged (dashed and per-cell pieces join up), lines that touch form a
//! component, and a component with at least two rows and two columns is a
//! grid. A missing line between two neighbouring grid cells merges them into
//! one spanning cell. Glyphs are placed in cells by their centre, and each
//! cell's text is laid out on its own, so a cell may hold several lines.

mod cells;
mod geometry;
mod grid;

use crate::extract::{Glyph, Rule};
use crate::tables::Grid;

use self::geometry::{components, merge, Line};
use self::grid::grid_of;

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
    /// An embedded image, as the Markdown that points at it.
    Image(String),
}

/// Find ruled tables. Returns the tables and the glyphs that are not inside
/// any of them.
pub(crate) fn ruled_tables(rules: &[Rule], glyphs: Vec<Glyph>) -> (Vec<RuledTable>, Vec<Glyph>) {
    let horizontal = merge(
        rules
            .iter()
            .filter(|r| r.horizontal)
            .map(|r| Line {
                at: r.at,
                from: r.from,
                to: r.to,
                soft: r.soft,
            })
            .collect(),
    );
    let vertical = merge(
        rules
            .iter()
            .filter(|r| !r.horizontal)
            .map(|r| Line {
                at: r.at,
                from: r.from,
                to: r.to,
                soft: r.soft,
            })
            .collect(),
    );
    if horizontal.len() < 2
        || vertical.is_empty()
        || horizontal.len().saturating_mul(vertical.len()) > MAX_PAIRS
    {
        return (Vec::new(), glyphs);
    }
    let components = components(&horizontal, &vertical);

    let mut tables = Vec::new();
    let mut claimed: Vec<bool> = vec![false; glyphs.len()];
    for (hs, vs) in components {
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
