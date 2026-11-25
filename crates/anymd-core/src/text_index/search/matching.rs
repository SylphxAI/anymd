use crate::text_index::TextIndexError;

use super::{normalize_for_search, NormalizedUtf16Offsets, SourceUtf16Projection};

pub(in crate::text_index) fn is_word_char(value: Option<char>) -> bool {
    matches!(value, Some(ch) if ch.is_ascii_alphanumeric() || ch == '_')
}

pub(in crate::text_index) fn is_whole_word_match(text: &str, start: usize, end: usize) -> bool {
    let before = text.get(..start).and_then(|s| s.chars().last());
    let after = text.get(end..).and_then(|s| s.chars().next());
    !is_word_char(before) && !is_word_char(after)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::text_index) struct TextMatchRange {
    pub(in crate::text_index) source_start: usize,
    pub(in crate::text_index) source_end: usize,
    pub(in crate::text_index) start_utf16: u32,
    pub(in crate::text_index) end_utf16: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiteralTextMatch {
    pub(crate) text: String,
    pub(crate) snippet: String,
    pub(crate) start_utf16: u32,
    pub(crate) end_utf16: u32,
}

/// Match in the normalized UTF-16 index space used by TS v3.0.14, then apply
/// those offsets directly to the original text for its `slice` semantics.
/// Boundary tables keep projection bounded and fail closed where JavaScript
/// could create a lone surrogate that Rust strings cannot represent.
#[cfg(test)]
pub(in crate::text_index) fn find_matches_in_text(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<Vec<TextMatchRange>, TextIndexError> {
    let source_projection = SourceUtf16Projection::new(text);
    find_matches_in_text_bounded(
        text,
        query,
        case_sensitive,
        whole_word,
        usize::MAX,
        &source_projection,
    )
}

pub(in crate::text_index) fn find_matches_in_text_bounded(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    max_results: usize,
    source_projection: &SourceUtf16Projection,
) -> Result<Vec<TextMatchRange>, TextIndexError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    let searchable_text = normalize_for_search(text, case_sensitive);
    let searchable_query = normalize_for_search(query, case_sensitive);
    if searchable_query.is_empty() || searchable_text.len() < searchable_query.len() {
        return Ok(matches);
    }
    let normalized_offsets = NormalizedUtf16Offsets::new(&searchable_text);
    let mut search_from = 0usize;
    while search_from + searchable_query.len() <= searchable_text.len() {
        let Some(relative_start) = searchable_text[search_from..].find(&searchable_query) else {
            break;
        };
        let start = search_from + relative_start;
        let end = start + searchable_query.len();
        if !whole_word || is_whole_word_match(&searchable_text, start, end) {
            let start_utf16 = normalized_offsets
                .utf16_at_byte(start)
                .expect("substring search starts at a UTF-8 character boundary");
            let end_utf16 = normalized_offsets
                .utf16_at_byte(end)
                .expect("substring search ends at a UTF-8 character boundary");
            let source_start = source_projection.byte_at_utf16_clamped(
                start_utf16,
                "UTF-16 search projection would split an astral character.",
            )?;
            let source_end = source_projection.byte_at_utf16_clamped(
                end_utf16,
                "UTF-16 search projection would split an astral character.",
            )?;
            matches.push(TextMatchRange {
                source_start,
                source_end,
                start_utf16: start_utf16.try_into().map_err(|_| {
                    TextIndexError::invalid_request("UTF-16 search offset exceeds u32 range.")
                })?,
                end_utf16: end_utf16.try_into().map_err(|_| {
                    TextIndexError::invalid_request("UTF-16 search offset exceeds u32 range.")
                })?,
            });
            if matches.len() >= max_results {
                break;
            }
        }
        search_from = end.max(start + 1);
    }

    Ok(matches)
}
