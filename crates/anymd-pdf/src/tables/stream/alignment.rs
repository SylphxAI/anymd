//! Detect column gutters and place phrases into the columns they define.

use super::Line;
use crate::rows::{is_cjk, Segment};
use crate::tables::strip_leaders;

#[derive(Debug, Clone)]
struct Phrase {
    x0: f64,
    x1: f64,
    text: String,
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

pub(super) struct AlignedRows {
    pub(super) lines: Vec<Line>,
    pub(super) width: usize,
    pub(super) size: f64,
}

pub(super) fn align_rows(rows: &[Vec<Segment>], word_space: f64) -> Option<AlignedRows> {
    let size = median(rows.iter().flatten().map(|s| s.size).collect()).unwrap_or(10.0);
    // A column gap is clearly wider than a word space: half the font size,
    // or more where the page's own word spaces are wide (typewriter text).
    // Monospace text has wide spaces of its own, so there a gap of two
    // spaces is needed.
    let mono = rows
        .iter()
        .flatten()
        .filter(|s| s.mono == Some(true))
        .count()
        * 2
        > rows.iter().flatten().count();
    let min_gap = if mono {
        size * 1.2
    } else {
        size * 0.5_f64.max(word_space * 1.6)
    };
    let row_phrases: Vec<Vec<Phrase>> = rows.iter().map(|row| phrases(row, min_gap)).collect();
    let structure: Vec<&Phrase> = row_phrases
        .iter()
        .filter(|row| row.len() >= 2)
        .flatten()
        .collect();
    let structure_rows = row_phrases.iter().filter(|row| row.len() >= 2).count();
    if structure_rows < 2 {
        return None;
    }
    let allowed = if structure_rows >= 4 {
        (structure_rows / 10).max(1)
    } else {
        0
    };
    let gutters = gutters(&structure, allowed, min_gap * 0.5);
    if gutters.is_empty() {
        return None;
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
        return None;
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
            .filter(|(_, used)| **used)
            .map(|(cell, _)| strip_leaders(cell))
            .collect();
        for span in &mut line.spans {
            *span = (
                remap[span.0].min(width - 1),
                remap[span.1].min(width - 1),
                remap[span.2].min(width - 1),
            );
        }
        line.spans.retain(|(a, b, _)| b > a);
    }
    Some(AlignedRows { lines, width, size })
}
