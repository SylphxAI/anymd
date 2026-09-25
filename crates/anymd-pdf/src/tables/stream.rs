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
use crate::rows::{is_cjk, Segment};
use crate::tables::{first_data_row, is_numeric, join_cell_line, strip_leaders, Cell, Grid};

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
struct Phrase {
    x0: f64,
    x1: f64,
    text: String,
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

fn join_words(into: &mut String, word: &str) {
    if into.is_empty() {
        into.push_str(word);
        return;
    }
    let glue = into.chars().last().is_some_and(is_cjk) && word.chars().next().is_some_and(is_cjk);
    if !glue {
        into.push(' ');
    }
    into.push_str(word);
}

/// Split a row's segments into phrases at word gaps of at least `min_gap`.
fn phrases(row: &[Segment], min_gap: f64) -> Vec<Phrase> {
    let mut out = Vec::new();
    for segment in row {
        let mut current: Option<Phrase> = None;
        for word in &segment.words {
            match current.as_mut() {
                Some(phrase) if word.x0 - phrase.x1 < min_gap => {
                    join_words(&mut phrase.text, &word.text);
                    phrase.x1 = phrase.x1.max(word.x1);
                }
                _ => {
                    out.extend(current.take());
                    current = Some(Phrase {
                        x0: word.x0,
                        x1: word.x1,
                        text: word.text.clone(),
                    });
                }
            }
        }
        out.extend(current);
    }
    out
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

/// Gutters: x ranges covered by at most `allowed` phrases.
fn gutters(phrases: &[&Phrase], allowed: usize, min_width: f64) -> Vec<(f64, f64)> {
    let mut events: Vec<(f64, i32)> = Vec::with_capacity(phrases.len() * 2);
    for phrase in phrases {
        events.push((phrase.x0, 1));
        events.push((phrase.x1, -1));
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out = Vec::new();
    let mut active = 0i32;
    let mut open: Option<f64> = None;
    let mut seen_text = false;
    for (x, delta) in events {
        let before = active;
        active += delta;
        if before as usize > allowed && active as usize <= allowed {
            open = Some(x);
            seen_text = true;
        } else if before as usize <= allowed && active as usize > allowed {
            if let Some(start) = open.take() {
                if seen_text && x - start >= min_width {
                    out.push((start, x));
                }
            }
        }
    }
    out
}

fn ends_open(text: &str) -> bool {
    const LINKS: &[&str] = &[
        "and", "or", "of", "the", "for", "to", "in", "with", "by", "on", "at", "from", "a", "an", "&",
    ];
    let text = text.trim_end();
    if text.ends_with('-') || text.ends_with(',') || text.ends_with('&') {
        return true;
    }
    let last = text.rsplit(' ').next().unwrap_or("").to_lowercase();
    LINKS.contains(&last.as_str())
}

fn starts_lower(text: &str) -> bool {
    text.trim_start().chars().next().is_some_and(char::is_lowercase)
}

fn append(cell: &mut String, text: &str) {
    join_cell_line(cell, text);
}

/// Build a table (or recognise side-by-side prose) from rows that each have
/// several aligned segments. `rules` are the page's ruling lines.
pub(crate) fn stream_table(rows: &[Vec<Segment>], rules: &[Rule]) -> Stream {
    let size = median(rows.iter().flatten().map(|s| s.size).collect()).unwrap_or(10.0);
    // A column gap is clearly wider than a word space.
    let mut spaces = Vec::new();
    for segment in rows.iter().flatten() {
        for pair in segment.words.windows(2) {
            let gap = pair[1].x0 - pair[0].x1;
            if gap > 0.0 && gap < size {
                spaces.push(gap);
            }
        }
    }
    let space = median(spaces).unwrap_or(size * 0.25);
    let min_gap = (space * 2.0).max(size * 0.4);
    let row_phrases: Vec<Vec<Phrase>> = rows.iter().map(|row| phrases(row, min_gap)).collect();
    let structure: Vec<&Phrase> = row_phrases
        .iter()
        .filter(|row| row.len() >= 2)
        .flatten()
        .collect();
    let structure_rows = row_phrases.iter().filter(|row| row.len() >= 2).count();
    if structure_rows < 2 {
        return Stream::Nothing;
    }
    let allowed = if structure_rows >= 4 {
        (structure_rows / 10).max(1)
    } else {
        0
    };
    let gutters = gutters(&structure, allowed, min_gap * 0.5);
    if gutters.is_empty() {
        return Stream::Nothing;
    }
    let mids: Vec<f64> = gutters.iter().map(|(a, b)| (a + b) / 2.0).collect();
    let column_of = |x: f64| mids.iter().take_while(|mid| **mid < x).count();
    let width = mids.len() + 1;

    let mut lines: Vec<Line> = Vec::with_capacity(rows.len());
    for (row, phrases) in rows.iter().zip(&row_phrases) {
        let mut cells = vec![String::new(); width];
        let mut spans = Vec::new();
        for phrase in phrases {
            let column = column_of((phrase.x0 + phrase.x1) / 2.0);
            let first = column_of(phrase.x0);
            let last = column_of(phrase.x1);
            if last > first {
                spans.push((first, last, column));
            }
            join_words(&mut cells[column], &phrase.text);
        }
        lines.push(Line {
            cells,
            spans,
            top: row.iter().map(|s| s.top).fold(f64::NEG_INFINITY, f64::max),
            bottom: row.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min),
        });
    }
    // Columns no row uses (a gutter inside a column's ragged edge).
    let used: Vec<bool> = (0..width)
        .map(|c| lines.iter().any(|line| !line.cells[c].is_empty()))
        .collect();
    if used.iter().filter(|u| **u).count() < 2 {
        return Stream::Nothing;
    }
    let remap: Vec<usize> = used
        .iter()
        .scan(0usize, |next, &u| {
            let index = *next;
            if u {
                *next += 1;
            }
            Some(index)
        })
        .collect();
    let width = used.iter().filter(|u| **u).count();
    for line in &mut lines {
        line.cells = line
            .cells
            .iter()
            .zip(&used)
            .filter_map(|(cell, u)| u.then(|| strip_leaders(cell)))
            .collect();
        for span in &mut line.spans {
            *span = (remap[span.0].min(width - 1), remap[span.1].min(width - 1), remap[span.2].min(width - 1));
        }
        line.spans.retain(|(a, b, _)| b > a);
    }

    if let Some(columns) = prose_columns(&lines) {
        return Stream::Columns(columns);
    }

    let x0 = rows.iter().flatten().map(|s| s.x0).fold(f64::INFINITY, f64::min);
    let x1 = rows.iter().flatten().map(|s| s.x1).fold(f64::NEG_INFINITY, f64::max);
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

    if std::env::var("ANYMD_DEBUG_STREAM").is_ok() { for l in &lines { eprintln!("top={:.1} bot={:.1} size={size:.1} {:?}", l.top, l.bottom, l.cells); } eprintln!("----"); }
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
    let separated_rows = lines
        .windows(2)
        .filter(|pair| ruled_between(&pair[0], &pair[1]))
        .count();

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
        // The rest of a description whose first column is blank.
        let continues_cells = first.is_empty()
            && !any_number
            && !filled.is_empty()
            && filled.iter().all(|c| !prev.cells[*c].is_empty());
        // The first line of a wrapped label, with the values on the next line.
        let label_head = prev_filled == [0]
            && !prev_number
            && !first.is_empty()
            && (starts_lower(first) || ends_open(&prev.cells[0]));
        // The rest of a wrapped label, below the line with the values.
        let label_tail = filled == [0]
            && !prev.cells[0].is_empty()
            && !any_number
            && (starts_lower(first) || ends_open(&prev.cells[0]));
        // Rows separated by rules: everything between two rules is one row.
        let same_band = separated_rows >= 2 && !any_number;
        if continues_cells || label_head || label_tail || same_band {
            for (c, text) in line.cells.iter().enumerate() {
                append(&mut prev.cells[c], text);
            }
            prev.bottom = prev.bottom.min(line.bottom);
        } else {
            merged.push(line);
        }
    }
    if merged.len() < 2 {
        return Stream::Nothing;
    }
    let cells = merged
        .iter()
        .enumerate()
        .map(|(r, line)| {
            let mut row: Vec<Option<Cell>> = line
                .cells
                .iter()
                .map(|text| Some(Cell { text: text.clone(), cols: 1, rows: 1 }))
                .collect();
            if r < header_rows {
                // A header phrase over several columns covers each of them.
                for &(first, last, placed) in &line.spans {
                    let text = row[placed].as_ref().map(|c| c.text.clone()).unwrap_or_default();
                    if (first..=last).all(|c| c == placed || line.cells[c].is_empty()) {
                        row[placed] = Some(Cell::default());
                        row[first] = Some(Cell { text, cols: last - first + 1, rows: 1 });
                        for slot in row.iter_mut().take(last + 1).skip(first + 1) {
                            *slot = None;
                        }
                    }
                }
            }
            row
        })
        .collect();
    Stream::Table(Grid { cells, header_rows })
}

/// Side-by-side columns of running text rather than a table: few columns of
/// long, wordy cells whose lines run on into the next row's cell.
fn prose_columns(lines: &[Line]) -> Option<Vec<Vec<String>>> {
    let width = lines.first()?.cells.len();
    if lines.len() < 3 || width > 4 {
        return None;
    }
    let cells: Vec<&String> = lines.iter().flat_map(|l| &l.cells).filter(|c| !c.is_empty()).collect();
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
