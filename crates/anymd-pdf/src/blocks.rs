//! Paragraphs, headings, list items and tables from ordered segments.

use std::collections::HashSet;

use crate::extract::{Glyph, RawPage, Rule};
use crate::margins::{in_margin, is_page_number, margin_key};
use crate::reading::reading_regions;
use crate::rows::{is_cjk, row_text, rows_of, segments_of_row, Segment};
use crate::tables::ruled::{ruled_tables, Ruled, RuledTable};
use crate::tables::stream::{stream_table, Stream};

#[derive(Debug, Clone)]
pub(crate) enum Block {
    Paragraph {
        text: String,
        size: f64,
        lines: usize,
    },
    ListItem(String),
    Table(Vec<Vec<String>>),
    Comment(String),
}

pub(crate) fn group_rows(mut segments: Vec<Segment>) -> Vec<Vec<Segment>> {
    segments.sort_by(|a, b| b.base.total_cmp(&a.base).then(a.x0.total_cmp(&b.x0)));
    let mut rows: Vec<Vec<Segment>> = Vec::new();
    for segment in segments {
        if let Some(row) = rows.last_mut() {
            let anchor = &row[0];
            let table = anchor.table.is_some() || segment.table.is_some();
            if !table && (anchor.base - segment.base).abs() <= 0.45 * anchor.size.max(segment.size) {
                row.push(segment);
                continue;
            }
        }
        rows.push(vec![segment]);
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    }
    rows
}

pub(crate) fn is_equation_number(text: &str) -> bool {
    let t = text.trim();
    t.len() <= 8
        && t.starts_with('(')
        && t.ends_with(')')
        && t[1..t.len() - 1]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
}

pub(crate) fn bullet_body(text: &str) -> Option<&str> {
    for bullet in [
        "•", "◦", "▪", "‣", "●", "○", "■", "□", "–", "—", "-", "*", "·", "\u{F0B7}", "➢", "✓",
    ] {
        if let Some(rest) = text.strip_prefix(bullet) {
            if rest.starts_with(' ')
                || (bullet != "-"
                    && bullet != "*"
                    && bullet != "–"
                    && bullet != "—"
                    && !rest.is_empty())
            {
                let body = rest.trim_start();
                if !body.is_empty() {
                    return Some(body);
                }
            }
        }
    }
    None
}

pub(crate) fn starts_enumerated(text: &str) -> bool {
    let mut chars = text.chars();
    let first = chars.next();
    match first {
        Some('(') => {
            let inner: String = chars.by_ref().take_while(|c| *c != ')').collect();
            !inner.is_empty()
                && inner.len() <= 4
                && inner.chars().all(|c| c.is_ascii_alphanumeric())
        }
        Some(c) if c.is_ascii_digit() || c.is_ascii_lowercase() => {
            let head: String = text
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            let rest = &text[head.len()..];
            head.len() <= 3
                && (head.chars().all(|c| c.is_ascii_digit()) || head.len() == 1)
                && (rest.starts_with(". ") || rest.starts_with(") "))
        }
        _ => false,
    }
}

/// Section headings like "3.2 Attention" or "4 Why Self-Attention".
pub(crate) fn numbered_heading_level(text: &str) -> Option<usize> {
    let text = text.trim();
    let (number, rest) = text.split_once(' ')?;
    let number = number.trim_end_matches('.');
    if number.is_empty() || number.len() > 8 {
        return None;
    }
    let parts: Vec<&str> = number.split('.').collect();
    let numeric = parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    let lettered =
        parts.len() == 1 && parts[0].len() == 1 && parts[0].chars().all(|c| c.is_ascii_uppercase());
    if !(numeric || lettered) || parts.len() > 4 {
        return None;
    }
    if numeric && parts[0].parse::<u32>().ok()? > 30 {
        return None;
    }
    let rest = rest.trim();
    let words = rest.split_whitespace().count();
    let first = rest.chars().next()?;
    if !first.is_uppercase() || words == 0 || words > 12 || rest.len() > 90 {
        return None;
    }
    if rest.ends_with('.') || rest.ends_with(',') || rest.ends_with(':') && words > 6 {
        return None;
    }
    // Headings are mostly letters, not numbers or math.
    let letters = rest.chars().filter(|c| c.is_alphabetic()).count();
    if letters * 10 < rest.chars().filter(|c| !c.is_whitespace()).count() * 7 {
        return None;
    }
    Some((parts.len() + 1).min(6))
}

pub(crate) fn named_heading(text: &str) -> bool {
    const NAMES: &[&str] = &[
        "abstract",
        "introduction",
        "background",
        "related work",
        "method",
        "methods",
        "methodology",
        "results",
        "discussion",
        "conclusion",
        "conclusions",
        "references",
        "bibliography",
        "acknowledgments",
        "acknowledgements",
        "appendix",
        "summary",
        "contents",
        "table of contents",
        "preface",
        "foreword",
        "index",
        "glossary",
    ];
    let lower = text.trim().trim_end_matches(':').to_lowercase();
    NAMES.contains(&lower.as_str())
}

pub(crate) fn join_line(paragraph: &mut String, line: &str) {
    if paragraph.is_empty() {
        paragraph.push_str(line);
        return;
    }
    let prev_last = paragraph.chars().last();
    let next_first = line.chars().next();
    if paragraph.ends_with('-') {
        let mut chars = paragraph.chars().rev();
        chars.next();
        let before = chars.next();
        let next_word = line.split_whitespace().next().unwrap_or("");
        let prev_word = paragraph.split_whitespace().next_back().unwrap_or("");
        let compound = prev_word[..prev_word.len() - 1].contains('-') || next_word.contains('-');
        if before.is_some_and(char::is_alphabetic) && next_first.is_some_and(char::is_lowercase) {
            if !compound {
                // A word broken across lines: "transduc-" + "tion".
                paragraph.pop();
            }
            // Compounds keep the hyphen: "left-to-" + "right".
            paragraph.push_str(line);
            return;
        }
    }
    if !(prev_last.is_some_and(is_cjk) && next_first.is_some_and(is_cjk)) {
        paragraph.push(' ');
    }
    paragraph.push_str(line);
}

/// What a page's regions share while they are turned into blocks: the ruled
/// tables already found (taken by their placeholder segments) and the
/// page's ruling lines.
pub(crate) struct PageTables {
    pub(crate) found: Vec<Option<RuledTable>>,
    pub(crate) rules: Vec<Rule>,
    /// The page's typical word space, in font sizes.
    pub(crate) word_space: f64,
}

impl PageTables {
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self {
            found: Vec::new(),
            rules: Vec::new(),
            word_space: 0.25,
        }
    }
}

pub(crate) fn layout_page(
    glyphs: &[Glyph],
    page: &RawPage,
    body: f64,
    repeated: &HashSet<String>,
) -> Vec<Block> {
    let (ruled, glyphs) = ruled_tables(&page.rules, glyphs.to_vec());
    let mut segments = Vec::new();
    for row in rows_of(glyphs) {
        segments.extend(segments_of_row(row));
    }
    if page.ocr {
        for segment in &mut segments {
            segment.mono = None;
        }
    }
    segments.retain(|segment| {
        !(in_margin(segment, page)
            && (repeated.contains(&margin_key(&segment.text)) || is_page_number(&segment.text)))
    });
    // Each ruled table takes part in reading order as one placeholder.
    for (index, table) in ruled.iter().enumerate() {
        segments.push(Segment {
            x0: table.x0,
            x1: table.x1,
            base: table.top - body,
            top: table.top,
            bottom: table.bottom,
            size: body,
            text: String::new(),
            mono: None,
            words: Vec::new(),
            table: Some(index),
        });
    }
    let mut spaces: Vec<f64> = segments
        .iter()
        .flat_map(|s| {
            s.words
                .windows(2)
                .map(move |pair| (pair[1].x0 - pair[0].x1) / s.size.max(0.1))
        })
        .filter(|gap| *gap > 0.0 && *gap < 1.2)
        .collect();
    spaces.sort_by(f64::total_cmp);
    let mut tables = PageTables {
        found: ruled.into_iter().map(Some).collect(),
        rules: page.rules.clone(),
        word_space: spaces.get(spaces.len() / 2).copied().unwrap_or(0.25),
    };
    let mut blocks = Vec::new();
    for region in reading_regions(segments, body, 0) {
        region_blocks(region, body, &mut tables, &mut blocks);
    }
    if !page.rotated.is_empty() {
        let mut lines = Vec::new();
        for row in rows_of(page.rotated.clone()) {
            let text = row_text(&segments_of_row(row));
            if !text.is_empty() {
                lines.push(text);
            }
        }
        if !lines.is_empty() {
            blocks.push(Block::Paragraph {
                text: lines.join(" "),
                size: 0.0,
                lines: 1,
            });
        }
    }
    blocks
}

/// How far above a table's first aligned row its header starts: short
/// lines just above it (the top lines of stacked column headings) that sit
/// tight above the next line, within the table's width, and are still in the
/// open paragraph.
fn header_lines_above(rows: &[Vec<Segment>], index: usize, end: usize, marks: &[(usize, usize)]) -> usize {
    let lo = rows[index..end].iter().flatten().map(|s| s.x0).fold(f64::INFINITY, f64::min);
    let hi = rows[index..end].iter().flatten().map(|s| s.x1).fold(f64::NEG_INFINITY, f64::max);
    let mut start = index;
    for &(row_index, _) in marks.iter().rev() {
        if row_index + 1 != start || index - row_index > 3 {
            break;
        }
        let row = &rows[row_index];
        let below = &rows[start];
        let size = row.iter().map(|s| s.size).fold(0.0, f64::max);
        let bottom = row.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min);
        let top = below.iter().map(|s| s.top).fold(f64::NEG_INFINITY, f64::max);
        let short = row.iter().all(|s| s.chars() <= 40);
        let inside = row.iter().all(|s| s.x0 >= lo - size && s.x1 <= hi + size);
        let text = row_text(row);
        if !(short && inside && bottom - top <= size * 0.8) || bullet_body(&text).is_some() {
            break;
        }
        start = row_index;
    }
    // Only a paragraph made of nothing but these lines: a short last line of
    // running text (a caption's tail) is not a header.
    match marks.first() {
        Some(&(first, _)) if first == start => start,
        _ => index,
    }
}

/// Blocks for side-by-side columns of running text: each column's lines
/// joined into paragraphs, left column first.
fn column_blocks(columns: Vec<Vec<String>>, blocks: &mut Vec<Block>) {
    for column in columns {
        let mut paragraph = String::new();
        let mut lines = 0;
        for line in column.into_iter().chain([String::new()]) {
            if line.is_empty() {
                if !paragraph.is_empty() {
                    blocks.push(Block::Paragraph {
                        text: std::mem::take(&mut paragraph),
                        size: 0.0,
                        lines,
                    });
                }
                lines = 0;
                continue;
            }
            join_line(&mut paragraph, &line);
            lines += 1;
        }
    }
}

pub(crate) fn region_blocks(
    region: Vec<Segment>,
    body: f64,
    tables: &mut PageTables,
    blocks: &mut Vec<Block>,
) {
    let mut rows = group_rows(region);
    if rows.is_empty() {
        return;
    }
    let left = rows.iter().map(|r| r[0].x0).fold(f64::INFINITY, f64::min);
    let right = rows
        .iter()
        .map(|r| r.last().map_or(f64::NEG_INFINITY, |s| s.x1))
        .fold(f64::NEG_INFINITY, f64::max);
    let is_multi = |row: &Vec<Segment>| {
        row.len() >= 2
            && !(row.len() == 2 && is_equation_number(&row[1].text))
            && row.iter().all(|s| s.table.is_none())
    };

    let mut index = 0;
    let mut paragraph = String::new();
    let mut paragraph_size = 0.0;
    let mut paragraph_lines = 0usize;
    let mut in_list = false;
    let mut prev: Option<(f64, f64, f64)> = None; // (bottom, x1, size) of previous line
    let mut continuation_x0 = f64::NAN;
    let mut para_right = f64::NEG_INFINITY;
    let mut prev_mono = false;
    let mut prev_x0 = f64::NAN;
    // (row index, paragraph length before it) for the rows of the open
    // paragraph, so a table can take back the header lines above it.
    let mut marks: Vec<(usize, usize)> = Vec::new();

    let flush =
        |blocks: &mut Vec<Block>, paragraph: &mut String, size: f64, lines: usize, list: bool| {
            if paragraph.is_empty() {
                return;
            }
            let text = std::mem::take(paragraph);
            blocks.push(if list {
                Block::ListItem(text)
            } else {
                Block::Paragraph { text, size, lines }
            });
        };

    while index < rows.len() {
        // A ruled table found earlier takes its place in reading order here.
        if let Some(slot) = rows[index].iter().find_map(|s| s.table) {
            flush(
                blocks,
                &mut paragraph,
                paragraph_size,
                paragraph_lines,
                in_list,
            );
            in_list = false;
            paragraph_lines = 0;
            if let Some(table) = tables.found.get_mut(slot).and_then(Option::take) {
                match table.content {
                    Ruled::Table { caption, grid } => {
                        if let Some(caption) = caption {
                            blocks.push(Block::Paragraph {
                                text: caption,
                                size: 0.0,
                                lines: 1,
                            });
                        }
                        blocks.push(Block::Table(grid.into_rows()));
                    }
                    Ruled::Frame(boxes) => {
                        for glyphs in boxes {
                            let segments: Vec<Segment> =
                                rows_of(glyphs).into_iter().flat_map(segments_of_row).collect();
                            let mut inner = PageTables {
                                found: Vec::new(),
                                rules: tables.rules.clone(),
                                word_space: tables.word_space,
                            };
                            for region in reading_regions(segments, body, 0) {
                                region_blocks(region, body, &mut inner, blocks);
                            }
                        }
                    }
                }
            }
            // Text beside the table on its first line stays as its own row.
            let rest: Vec<Segment> = rows[index].iter().filter(|s| s.table.is_none()).cloned().collect();
            prev = None;
            if rest.is_empty() {
                index += 1;
                continue;
            }
            rows[index] = rest;
        }
        // Tables: runs of rows with several aligned cells.
        if is_multi(&rows[index]) {
            let mut end = index;
            let mut multi = 0;
            // Column anchors (cell starts and ends) seen so far in this run.
            let mut anchors: Vec<f64> = Vec::new();
            let aligned = |row: &Vec<Segment>, anchors: &[f64]| {
                if anchors.is_empty() {
                    return true;
                }
                let tolerance = row[0].size * 0.8;
                let hits = row
                    .iter()
                    .filter(|s| {
                        anchors
                            .iter()
                            .any(|a| (a - s.x0).abs() <= tolerance || (a - s.x1).abs() <= tolerance)
                    })
                    .count();
                hits * 2 >= row.len()
            };
            while end < rows.len() {
                if is_multi(&rows[end]) {
                    if multi >= 2 && !aligned(&rows[end], &anchors) {
                        // A different grid starts here (e.g. a second author block).
                        break;
                    }
                    anchors.extend(rows[end].iter().flat_map(|s| [s.x0, s.x1]));
                    multi += 1;
                    end += 1;
                } else if multi > 0 {
                    // Short lines between aligned rows (wrapped labels) stay
                    // in the table when an aligned row follows within a few
                    // lines, or when they sit tight under the last one at
                    // the table's left edge.
                    let left = rows[index..end].iter().map(|r| r[0].x0).fold(f64::INFINITY, f64::min);
                    let bottom_of = |row: &Vec<Segment>| row.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min);
                    // A wrapped label: one short line at the table's left
                    // edge, tight under the line above it.
                    let label_line = |r: usize| {
                        let row = &rows[r];
                        let size = row[0].size;
                        row.len() == 1
                            && row[0].chars() <= 40
                            && (row[0].x0 - left).abs() <= size * 1.5
                            && bottom_of(&rows[r - 1]) - row[0].top <= size * 0.45
                    };
                    let ahead = (end..rows.len().min(end + 4))
                        .take_while(|&r| !is_multi(&rows[r]) && label_line(r))
                        .count();
                    if ahead > 0 && end + ahead < rows.len() && is_multi(&rows[end + ahead]) {
                        end += ahead;
                        continue;
                    }
                    if ahead > 0 {
                        end += ahead;
                    }
                    break;
                } else {
                    break;
                }
            }
            if multi >= 2 {
                let start = header_lines_above(&rows, index, end, &marks);
                let found = stream_table(&rows[start..end], &tables.rules, tables.word_space);
                if found != Stream::Nothing {
                    if start < index {
                        let (_, len) = marks[marks.len() - (index - start)];
                        paragraph.truncate(len);
                        paragraph_lines = paragraph_lines.saturating_sub(index - start);
                    }
                    marks.clear();
                    flush(
                        blocks,
                        &mut paragraph,
                        paragraph_size,
                        paragraph_lines,
                        in_list,
                    );
                    in_list = false;
                    paragraph_lines = 0;
                    match found {
                        Stream::Table(grid) => blocks.push(Block::Table(grid.into_rows())),
                        Stream::Columns(columns) => column_blocks(columns, blocks),
                        Stream::Nothing => {}
                    }
                    let last = &rows[end - 1];
                    prev = Some((
                        last.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min),
                        last.last().map_or(0.0, |s| s.x1),
                        last[0].size,
                    ));
                    index = end;
                    continue;
                }
            }
        }
        let row = &rows[index];
        let text = row_text(row);
        let size = row.iter().map(|s| s.size).fold(0.0, f64::max);
        let top = row.iter().map(|s| s.top).fold(f64::NEG_INFINITY, f64::max);
        let bottom = row.iter().map(|s| s.bottom).fold(f64::INFINITY, f64::min);
        let x0 = row[0].x0;
        let x1 = row.last().map_or(x0, |s| s.x1);
        let bullet = bullet_body(&text);
        let enumerated = starts_enumerated(&text);
        // A numbered line that fills the region and runs on into a lowercase
        // next line is the first line of a numbered paragraph, not a heading.
        let runs_on = x1 - x0 >= (right - left) * 0.85
            && rows.get(index + 1).is_some_and(|next| {
                next.iter().all(|s| s.table.is_none())
                    && row_text(next).chars().next().is_some_and(char::is_lowercase)
            });
        let heading_like =
            (numbered_heading_level(&text).is_some() || named_heading(&text)) && !runs_on;
        // Monospace text (receipts, code, terminal output) keeps its line breaks.
        let determined = row.iter().any(|s| s.mono.is_some());
        let row_mono = if determined {
            row.iter().any(|s| s.mono == Some(true)) && !row.iter().any(|s| s.mono == Some(false))
        } else {
            prev_mono
        };

        let new_block = match prev {
            None => true,
            Some((prev_bottom, prev_x1, prev_size)) => {
                let gap = prev_bottom - top;
                let size_change = (size - prev_size).abs() > prev_size.max(size) * 0.12;
                let heading_continues =
                    size >= body * 1.15 && !size_change && gap <= size * 0.6 && paragraph_lines < 3;
                // Short against its own paragraph (or the next line), or a short
                // stand-alone line in a wide region (key: value rows).
                let prev_width = prev_x1 - prev_x0;
                let prev_short = !heading_continues
                    && (prev_x1 < para_right.max(x1) - prev_size * 2.5
                        || (prev_x1 < right - prev_size * 2.5 && prev_width < (right - left) * 0.6));
                let indented = x0 > continuation_x0 + size * 0.8 && !in_list;
                let outdented = paragraph_lines >= 2 && x0 < continuation_x0 - size * 0.8;
                // Centered lines (title blocks, letterheads) stay separate.
                let region_width = right - left;
                let centered = !heading_continues
                    && prev_width < region_width * 0.8
                    && x1 - x0 < region_width * 0.8
                    && ((prev_x0 + prev_x1) / 2.0 - (x0 + x1) / 2.0).abs() < size * 0.6
                    && (prev_x0 - x0).abs() > size * 0.8
                    && x0.min(prev_x0) > left + size;
                gap > size.max(prev_size) * 0.55
                    || centered
                    || outdented
                    || size_change
                    || prev_short
                    || indented
                    || bullet.is_some()
                    || enumerated
                    || heading_like
            }
        };
        if new_block {
            flush(
                blocks,
                &mut paragraph,
                paragraph_size,
                paragraph_lines,
                in_list,
            );
            paragraph_lines = 0;
            paragraph_size = size;
            in_list = bullet.is_some();
            // A list item's continuation lines are indented under the bullet.
        } else if in_list && x0 <= left + size * 0.3 && bullet.is_none() {
            // Back at the margin: the list item ended.
            flush(
                blocks,
                &mut paragraph,
                paragraph_size,
                paragraph_lines,
                true,
            );
            paragraph_lines = 0;
            paragraph_size = size;
            in_list = false;
        }
        if paragraph_lines == 0 {
            para_right = f64::NEG_INFINITY;
        }
        if paragraph.is_empty() {
            marks.clear();
        }
        marks.push((index, paragraph.len()));
        if row_mono && prev_mono && !paragraph.is_empty() && !in_list {
            paragraph.push('\n');
            paragraph.push_str(&text);
        } else {
            join_line(&mut paragraph, bullet.unwrap_or(&text));
        }
        prev_mono = row_mono;
        paragraph_lines += 1;
        para_right = para_right.max(x1);
        prev_x0 = x0;
        if paragraph_lines == 2 {
            continuation_x0 = x0;
        } else if paragraph_lines == 1 {
            continuation_x0 = f64::NAN;
        }
        prev = Some((bottom, x1, size));
        // A heading line stands alone.
        if heading_like && !in_list {
            flush(
                blocks,
                &mut paragraph,
                paragraph_size,
                paragraph_lines,
                false,
            );
            paragraph_lines = 0;
            prev = Some((bottom, f64::NEG_INFINITY, size));
        }
        index += 1;
    }
    flush(
        blocks,
        &mut paragraph,
        paragraph_size,
        paragraph_lines,
        in_list,
    );
}
