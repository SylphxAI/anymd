//! Tables implied by aligned whitespace (Camelot's and Tabula's "stream"
//! mode, after Nurminen): rows of text whose gaps line up form columns.
//!
//! Words that sit closer than a column gap form a phrase. Column gutters are
//! the x ranges that (almost) no phrase of the table's rows covers, so a
//! column holds one phrase per row even when two columns sit closer than the
//! gap that splits a line into segments. Rows that only continue the cell
//! text of the row above (wrapped labels and descriptions) are merged into
//! it, stacked header rows become one header, and a header that spans
//! several columns is repeated over each of them.

use crate::extract::Rule;
use crate::rows::Segment;
use crate::tables::Grid;

mod alignment;
mod continuations;
mod grid;
mod prose;

/// What a run of aligned rows turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stream {
    /// A table.
    Table(Grid),
    /// Side-by-side columns of running text, left to right, each as its
    /// lines (an empty string where a column has a blank line).
    Columns(Vec<Vec<String>>),
    /// Neither: not enough aligned structure.
    Nothing,
}

#[derive(Debug, Clone)]
struct Line {
    cells: Vec<String>,
    /// Phrases that cross a gutter: (first column, last column, column the
    /// text was placed in).
    spans: Vec<(usize, usize, usize)>,
    top: f64,
    bottom: f64,
}

fn starts_lower(text: &str) -> bool {
    text.trim_start()
        .chars()
        .next()
        .is_some_and(char::is_lowercase)
}

/// Build a table (or recognise side-by-side prose) from rows that each have
/// several aligned segments. `rules` are the page's ruling lines.
pub(crate) fn stream_table(
    rows: &[Vec<Segment>],
    rules: &[Rule],
    word_space: f64,
    ocr: bool,
) -> Stream {
    let Some(aligned) = alignment::align_rows(rows, word_space) else {
        return Stream::Nothing;
    };
    if let Some(columns) = prose::prose_columns(&aligned.lines) {
        return Stream::Columns(columns);
    }
    grid::build_grid(rows, rules, aligned, ocr).map_or(Stream::Nothing, Stream::Table)
}
