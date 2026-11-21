//! Default agent-facing output: clean Markdown with page markers, a tiny
//! front-matter header, and a token budget with a continuation cursor.
//!
//! Evidence-heavy JSON (bounding boxes, hashes, document maps, audits) stays
//! available behind explicit options on the legacy routes.

use rmcp::model::{CallToolResult, ContentBlock};

mod read;
mod search;

pub use anymd_core::text_rules::estimate_tokens;

pub use read::pagination::{Cursor, DEFAULT_MAX_TOKENS, MAX_MAX_TOKENS, MIN_MAX_TOKENS};
pub use read::{read, read_pdf, read_text, read_wants_legacy, ReadRender};
pub use search::{search, search_pdf, search_wants_legacy};

pub(crate) use read::scan_marker;

/// Plain text of a tool result (CLI output).
pub fn result_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn finish(out: String, all_failed: bool) -> CallToolResult {
    let text = out.trim_end().to_string() + "\n";
    if all_failed {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    }
}

// Keep the original lean::tests::* paths for test filters.
#[cfg(test)]
mod tests;
