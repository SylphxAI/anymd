//! Paragraphs, headings, list items and tables from ordered segments.

mod lines;
mod page;
mod region;
mod table_runs;

use crate::extract::Rule;
use crate::tables::ruled::RuledTable;

pub(crate) use lines::{
    bullet_body, group_rows, is_equation_number, join_line, named_heading, numbered_heading_level,
    starts_enumerated,
};
pub(crate) use page::layout_page;
pub(crate) use region::region_blocks;

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
    /// An embedded image: Markdown that points at the exported file.
    Image(String),
}

/// What a page's regions share while they are turned into blocks: the ruled
/// tables already found (taken by their placeholder segments) and the
/// page's ruling lines.
pub(crate) struct PageTables {
    pub(crate) found: Vec<Option<RuledTable>>,
    pub(crate) rules: Vec<Rule>,
    /// The page's typical word space, in font sizes.
    pub(crate) word_space: f64,
    /// OCR word boxes need extra recovery for labels split across lines.
    pub(crate) ocr: bool,
}

impl PageTables {
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self {
            found: Vec::new(),
            rules: Vec::new(),
            word_space: 0.25,
            ocr: false,
        }
    }
}
