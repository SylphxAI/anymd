//! Distinguish side-by-side prose from tabular values.

use super::{starts_lower, Line};
use crate::tables::is_numeric;

/// Side-by-side columns of running text rather than a table: few columns of
/// long, wordy cells whose lines run on into the next row's cell.
pub(super) fn prose_columns(lines: &[Line]) -> Option<Vec<Vec<String>>> {
    let width = lines.first()?.cells.len();
    if lines.len() < 3 || width > 4 {
        return None;
    }
    let cells: Vec<&String> = lines
        .iter()
        .flat_map(|l| &l.cells)
        .filter(|c| !c.is_empty())
        .collect();
    let numeric = cells.iter().filter(|c| is_numeric(c)).count();
    if cells.is_empty() || numeric * 10 > cells.len() {
        return None;
    }
    let chars: usize = cells.iter().map(|c| c.chars().count()).sum();
    if chars / cells.len() < 25 {
        return None;
    }
    let mut prose_columns = 0;
    for c in 0..width {
        let (mut pairs, mut runs_on) = (0, 0);
        for pair in lines.windows(2) {
            let (upper, lower) = (&pair[0].cells[c], &pair[1].cells[c]);
            if upper.is_empty() || lower.is_empty() {
                continue;
            }
            pairs += 1;
            let open = !upper.ends_with(['.', ':', ';', '?', '!']);
            if open && (starts_lower(lower) || upper.ends_with(['-', ','])) {
                runs_on += 1;
            }
        }
        if pairs >= 2 && runs_on * 2 >= pairs {
            prose_columns += 1;
        }
    }
    (prose_columns >= 2 || (prose_columns >= 1 && width == 2)).then(|| {
        (0..width)
            .map(|c| lines.iter().map(|l| l.cells[c].clone()).collect())
            .collect()
    })
}
