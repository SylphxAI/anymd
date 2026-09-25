//! Tables: grids drawn with ruling lines (`ruled`) and grids implied by
//! aligned whitespace (`stream`), turned into one Markdown pipe table each.
//!
//! Both finders produce a [`Grid`]: rows of cells, where a cell may span
//! several columns (a spanning header) or rows. [`Grid::into_rows`] then
//! flattens stacked header rows into the single header row a pipe table has,
//! repeating a spanning header over each column it covers.

pub(crate) mod ruled;
pub(crate) mod stream;

#[cfg(test)]
mod tests;

/// One cell: its text and how many columns and rows it covers.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Cell {
    pub(crate) text: String,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
}

/// A table before it is rendered. `cells[r][c]` is `Some` where a cell starts
/// and `None` where a cell from above or from the left continues.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Grid {
    pub(crate) cells: Vec<Vec<Option<Cell>>>,
    /// How many leading rows are headers.
    pub(crate) header_rows: usize,
}

impl Grid {
    pub(crate) fn width(&self) -> usize {
        self.cells.first().map_or(0, Vec::len)
    }

    /// The text of each row, one string per column: a spanning header's text
    /// in every column it covers, a body cell's text in its first column.
    fn spread(&self) -> Vec<Vec<String>> {
        let width = self.width();
        let mut out = vec![vec![String::new(); width]; self.cells.len()];
        for (r, row) in self.cells.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let Some(cell) = cell else { continue };
                if r < self.header_rows {
                    let rows = cell.rows.max(1).min(self.header_rows - r);
                    for target in out.iter_mut().skip(r).take(rows) {
                        for slot in target.iter_mut().skip(c).take(cell.cols.max(1)) {
                            *slot = cell.text.clone();
                        }
                    }
                } else {
                    out[r][c] = cell.text.clone();
                }
            }
        }
        out
    }

    /// Rows for a pipe table: the header rows joined column by column into
    /// one, then the body rows.
    pub(crate) fn into_rows(self) -> Vec<Vec<String>> {
        let header_rows = self.header_rows.min(self.cells.len());
        let mut rows = self.spread();
        if header_rows >= 2 {
            let width = rows[0].len();
            let mut header = vec![String::new(); width];
            for row in &rows[..header_rows] {
                for (slot, text) in header.iter_mut().zip(row) {
                    if text.is_empty() || slot.ends_with(text.as_str()) {
                        continue;
                    }
                    join_cell_line(slot, text);
                }
            }
            rows.splice(..header_rows, [header]);
        }
        rows
    }
}

/// A number as tables print it: digits with separators, a sign, a currency,
/// a percent, parentheses for negatives, or a footnote mark.
pub(crate) fn is_numeric(text: &str) -> bool {
    let core = strip_marks(text);
    let core = core
        .trim_start_matches(['(', '$', '€', '£', '¥', '*', ' '])
        .trim_start_matches(['-', '−', '–', '+'])
        .trim_start_matches(['$', '€', '£', '¥', ' '])
        .trim_end_matches([')', '%', '*', ' ']);
    let mut digits = 0;
    for ch in core.chars() {
        match ch {
            '0'..='9' => digits += 1,
            '.' | ',' | ' ' | '\'' => {}
            _ => return false,
        }
    }
    digits > 0 && core.starts_with(|c: char| c.is_ascii_digit() || c == '.')
}

/// Join a cell's next line: a line-end hyphen stays (table headings break at
/// real hyphens: "House-" + "passed"), CJK text joins without a space.
pub(crate) fn join_cell_line(cell: &mut String, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let glue = cell.is_empty()
        || cell.ends_with('-')
        || (cell.chars().last().is_some_and(crate::rows::is_cjk)
            && line.chars().next().is_some_and(crate::rows::is_cjk));
    if !glue {
        cell.push(' ');
    }
    cell.push_str(line);
}

/// A year or year-like column label ("2025", "2026p", "FY2024"), which heads
/// a column rather than being data.
pub(crate) fn is_year(text: &str) -> bool {
    let core = strip_marks(text);
    let core = core.trim_start_matches("FY").trim();
    let digits: String = core.chars().take_while(char::is_ascii_digit).collect();
    let rest = &core[digits.len()..];
    digits.len() == 4
        && matches!(&digits[..2], "18" | "19" | "20" | "21")
        && rest.chars().all(|c| c.is_ascii_lowercase() || c == ' ')
        && rest.trim().len() <= 2
}

/// Text without trailing footnote marks written as `^x`.
fn strip_marks(text: &str) -> &str {
    let text = text.trim();
    match text.rfind('^') {
        Some(index) if text.len() - index <= 4 => text[..index].trim_end(),
        _ => text,
    }
}

/// The first body row: the first row after the first that has a number
/// (other than a year) outside the first column.
fn first_data_row(rows: &[Vec<String>]) -> Option<usize> {
    rows.iter().enumerate().skip(1).find_map(|(index, row)| {
        row.iter()
            .skip(1)
            .any(|cell| !cell.is_empty() && is_numeric(cell) && !is_year(cell))
            .then_some(index)
    })
}

/// Leader dots ("Total ........ 12") removed from a cell; short runs such as
/// an ellipsis stay.
pub(crate) fn strip_leaders(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let is_dot = |c: char| matches!(c, '.' | '·' | '…' | '_');
        if is_dot(chars[index]) {
            // A run of dots, possibly spaced (". . . .").
            let mut end = index;
            let mut dots = 0;
            while end < chars.len() && (is_dot(chars[end]) || (chars[end] == ' ' && end + 1 < chars.len() && is_dot(chars[end + 1]))) {
                if is_dot(chars[end]) {
                    dots += if chars[end] == '…' { 3 } else { 1 };
                }
                end += 1;
            }
            if dots >= 4 {
                index = end;
                continue;
            }
            out.extend(&chars[index..end]);
            index = end;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
