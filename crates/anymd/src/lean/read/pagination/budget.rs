//! Token limits, continuation notes, and UTF-8-safe cuts within a unit.

use super::{Cursor, DEFAULT_MAX_TOKENS, MAX_MAX_TOKENS, MIN_MAX_TOKENS};

pub(in crate::lean) fn budget_from(max_tokens: Option<u32>) -> usize {
    max_tokens
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_MAX_TOKENS)
        .clamp(MIN_MAX_TOKENS, MAX_MAX_TOKENS)
}

pub(in crate::lean) fn continuation_note(budget: usize, next: Cursor) -> String {
    if budget == usize::MAX {
        return format!("<!-- Continue with cursor: \"{}\" -->\n\n", next.render());
    }
    format!(
        "<!-- Stopped at the {budget}-token budget. Continue with cursor: \"{}\", or pick pages, or raise max_tokens. -->\n\n",
        next.render()
    )
}

/// Cut `text` at a paragraph (or line, or char) boundary no later than `limit` bytes.
pub(in crate::lean) fn cut_point(text: &str, limit: usize) -> usize {
    if text.len() <= limit {
        return text.len();
    }
    let mut boundary = limit;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let head = &text[..boundary];
    if let Some(index) = head.rfind("\n\n").filter(|index| *index > limit / 2) {
        return index + 2;
    }
    if let Some(index) = head.rfind('\n').filter(|index| *index > limit / 2) {
        return index + 1;
    }
    boundary.max(1)
}
