//! Compact pipe-table rendering and its bounded conversion path.

use crate::{limits, ConvertError};

/// Escape a table cell for a Markdown pipe table.
pub fn table_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace("\r\n", " ")
        .replace(['\n', '\r'], " ")
        .trim()
        .to_string()
}

/// Render rows as a Markdown pipe table; the first row is the header.
pub fn markdown_table(rows: &[Vec<String>]) -> String {
    render_markdown_table(rows, None).expect("unlimited table")
}

pub(crate) fn bounded_markdown_table(rows: &[Vec<String>]) -> Result<String, ConvertError> {
    render_markdown_table(rows, Some(limits::MAX_OUTPUT_BYTES))
}

fn render_markdown_table(
    rows: &[Vec<String>],
    limit: Option<usize>,
) -> Result<String, ConvertError> {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return Ok(String::new());
    }
    let mut out = String::new();
    // A ragged table can expand quadratically through empty-cell padding.
    // Refuse that expansion before iterating over the missing cells.
    if let Some(limit) = limit {
        let minimum = width
            .saturating_add(2)
            .saturating_mul(rows.len())
            .saturating_add(width.saturating_mul(2).saturating_add(2));
        if minimum > limit {
            return Err(ConvertError::limit("output", limit));
        }
    }
    for (index, row) in rows.iter().enumerate() {
        // Compact pipe tables: padding spaces cost ~20% more tokens.
        out.push('|');
        for column in 0..width {
            let cell = table_cell(row.get(column).map(String::as_str).unwrap_or(""));
            if let Some(limit) = limit {
                if cell.len() > limit.saturating_sub(out.len()) {
                    return Err(ConvertError::limit("output", limit));
                }
            }
            out.push_str(&cell);
            out.push('|');
        }
        out.push('\n');
        if index == 0 {
            out.push('|');
            for _ in 0..width {
                out.push_str("-|");
            }
            out.push('\n');
        }
    }
    if limit.is_some() {
        limits::check_output(&out)?;
    }
    Ok(out)
}
