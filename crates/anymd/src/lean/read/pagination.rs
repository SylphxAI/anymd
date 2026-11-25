//! Page selection, Markdown rendering and token-budgeted continuation of an opened document.

mod budget;
mod reader;
mod response;
mod selection;

pub(in crate::lean) use budget::{budget_from, continuation_note, cut_point};
pub(in crate::lean) use reader::read_opened;
pub(in crate::lean) use response::failed;
pub(in crate::lean) use selection::describe_pages;

/// Below common MCP client output caps (Claude Code rejects results over 25k tokens).
pub const DEFAULT_MAX_TOKENS: usize = 20_000;
pub const MIN_MAX_TOKENS: usize = 500;
pub const MAX_MAX_TOKENS: usize = 1_000_000;

/// Where a read resumes: 1-based page and a character offset inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub page: u32,
    pub offset: usize,
}

impl Cursor {
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim();
        let (page, offset) = match value.split_once(':') {
            Some((page, offset)) => (page, offset),
            None => (value, "0"),
        };
        let page = page
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|page| *page >= 1)
            .ok_or_else(|| {
                format!("Invalid cursor {value:?}: expected \"<page>\" or \"<page>:<offset>\".")
            })?;
        let offset = offset
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("Invalid cursor {value:?}: offset must be a number."))?;
        Ok(Self { page, offset })
    }

    pub(in crate::lean) fn render(self) -> String {
        if self.offset == 0 {
            self.page.to_string()
        } else {
            format!("{}:{}", self.page, self.offset)
        }
    }
}

pub(in crate::lean) struct SourceRead {
    pub(in crate::lean) header: Vec<(String, String)>,
    pub(in crate::lean) body: String,
    pub(in crate::lean) next: Option<Cursor>,
    pub(in crate::lean) shown: usize,
    pub(in crate::lean) error: Option<String>,
}
