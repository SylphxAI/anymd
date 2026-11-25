//! Assemble blocks in the reading order of a single region.

use crate::reading::reading_regions;
use crate::rows::{row_text, rows_of, segments_of_row, Segment};
use crate::tables::ruled::Ruled;
use crate::tables::stream::Stream;

use super::table_runs::{column_blocks, stream_run};
use super::{
    bullet_body, group_rows, join_line, named_heading, numbered_heading_level, starts_enumerated,
    Block, PageTables,
};

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
                    Ruled::Image(markdown) => blocks.push(Block::Image(markdown)),
                    Ruled::Frame(boxes) => {
                        for glyphs in boxes {
                            let segments: Vec<Segment> = rows_of(glyphs)
                                .into_iter()
                                .flat_map(segments_of_row)
                                .collect();
                            let mut inner = PageTables {
                                found: Vec::new(),
                                rules: tables.rules.clone(),
                                word_space: tables.word_space,
                                ocr: tables.ocr,
                            };
                            for region in reading_regions(segments, body, 0) {
                                region_blocks(region, body, &mut inner, blocks);
                            }
                        }
                    }
                }
            }
            // Text beside the table on its first line stays as its own row.
            let rest: Vec<Segment> = rows[index]
                .iter()
                .filter(|s| s.table.is_none())
                .cloned()
                .collect();
            prev = None;
            if rest.is_empty() {
                index += 1;
                continue;
            }
            rows[index] = rest;
        }
        // Tables: runs of rows with several aligned cells.
        if let Some((run, found)) = stream_run(&rows, index, &marks, tables) {
            let (start, end) = (run.start, run.end);
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
        let runs_on = x1 - x0 >= (right - left) * 0.6
            && rows.get(index + 1).is_some_and(|next| {
                next.iter().all(|s| s.table.is_none())
                    && row_text(next)
                        .chars()
                        .next()
                        .is_some_and(char::is_lowercase)
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
                        || (prev_x1 < right - prev_size * 2.5
                            && prev_width < (right - left) * 0.6));
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
