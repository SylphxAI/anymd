//! Blocks to Markdown.

use crate::blocks::{named_heading, numbered_heading_level, Block};

pub(crate) fn is_size_heading(size: f64, body: f64, text: &str) -> bool {
    size >= body * 1.15
        && text.chars().count() <= 120
        && text.chars().next().is_some_and(|c| !c.is_lowercase())
        && text.chars().any(char::is_alphabetic)
        && !text.ends_with(',')
}

/// Map distinct heading font sizes (largest first) to Markdown levels.
pub(crate) fn heading_levels(sizes: &[f64]) -> Vec<f64> {
    let mut distinct: Vec<f64> = Vec::new();
    let mut sorted = sizes.to_vec();
    sorted.sort_by(|a, b| b.total_cmp(a));
    for size in sorted {
        if distinct.last().is_none_or(|last| (last - size).abs() > 0.6) {
            distinct.push(size);
        }
    }
    distinct
}

pub(crate) fn level_for(size: f64, levels: &[f64]) -> usize {
    levels
        .iter()
        .position(|level| (level - size).abs() <= 0.6)
        .map_or(3, |index| (index + 1).min(4))
}

pub(crate) fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

pub(crate) fn render_blocks(
    blocks: &[Block],
    body: f64,
    levels: &[f64],
    first_heading: &mut Option<String>,
) -> String {
    let mut out = String::new();
    for block in blocks {
        let piece = match block {
            Block::Paragraph { text, size, lines } => {
                let numbered = if *lines == 1 {
                    numbered_heading_level(text).or_else(|| named_heading(text).then_some(2))
                } else {
                    None
                };
                let level = numbered.or_else(|| {
                    (is_size_heading(*size, body, text) && *lines <= 3)
                        .then(|| level_for(*size, levels))
                });
                match level {
                    Some(level) => {
                        if first_heading.is_none() && level == 1 {
                            *first_heading = Some(text.clone());
                        }
                        format!("{} {}", "#".repeat(level), text)
                    }
                    None => text.clone(),
                }
            }
            Block::ListItem(text) => format!("- {text}"),
            Block::Table(rows) => {
                let mut table = String::new();
                for (index, row) in rows.iter().enumerate() {
                    // Compact pipe tables: padding spaces cost ~20% more tokens.
                    table.push('|');
                    for cell in row {
                        table.push_str(&escape_cell(cell));
                        table.push('|');
                    }
                    table.push('\n');
                    if index == 0 {
                        table.push('|');
                        for _ in row {
                            table.push_str("-|");
                        }
                        table.push('\n');
                    }
                }
                table.trim_end().to_string()
            }
            Block::Comment(message) => format!("<!-- {message} -->"),
        };
        if piece.is_empty() {
            continue;
        }
        if !out.is_empty() {
            // Consecutive list items stay in one list.
            let list_continues = matches!(block, Block::ListItem(_)) && out.ends_with_list_item();
            out.push_str(if list_continues { "\n" } else { "\n\n" });
        }
        out.push_str(&piece);
    }
    out
}

pub(crate) trait EndsWithListItem {
    fn ends_with_list_item(&self) -> bool;
}

impl EndsWithListItem for String {
    fn ends_with_list_item(&self) -> bool {
        self.rsplit("\n\n")
            .next()
            .and_then(|last| last.lines().last())
            .is_some_and(|line| line.starts_with("- "))
    }
}
