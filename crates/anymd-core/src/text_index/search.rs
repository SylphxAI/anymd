//! Literal text search over extracted PDF pages: matching, whole-word checks, snippets and match geometry.

use super::*;

#[allow(clippy::too_many_arguments)]
pub fn search_pdf_text(
    path: &Path,
    max_file_bytes: u64,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    max_pages: u32,
    max_matches: u32,
    context_chars: u32,
) -> Result<TextSearchResult, TextIndexError> {
    search_pdf_text_pages(
        path,
        max_file_bytes,
        query,
        case_sensitive,
        whole_word,
        None,
        max_pages,
        max_matches,
        context_chars,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn search_pdf_text_pages(
    path: &Path,
    max_file_bytes: u64,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    requested_pages: Option<&[u32]>,
    max_pages: u32,
    max_matches: u32,
    context_chars: u32,
) -> Result<TextSearchResult, TextIndexError> {
    if query.is_empty() {
        return Err(TextIndexError::invalid_params("query must not be empty."));
    }

    // TS 3.0.14 does not persist extracted document text beside the source.
    // Keep search side-effect free; a future cache needs an explicit private-root contract.
    let extracted = extract_pdf_text(path, max_file_bytes)?;
    let pages = extracted.pages;
    let page_cache = None;
    let num_pages = pages.len().max(1) as u32;
    let searched_pages: Vec<u32> = requested_pages
        .map(|requested| requested.to_vec())
        .unwrap_or_else(|| (1..=num_pages).collect())
        .into_iter()
        .filter(|page| *page <= num_pages)
        .take(max_pages.max(1) as usize)
        .collect();
    let mut matches = Vec::new();
    let mut truncated = false;

    for &page in &searched_pages {
        let page_index = (page - 1) as usize;
        let page_text = pages.get(page_index);
        for (text_item_index, item) in page_text
            .map(|page| page.positioned_items.iter())
            .into_iter()
            .flatten()
            .enumerate()
        {
            let text = &item.text;
            let source_projection = SourceUtf16Projection::new(text);
            let geometry_index = TextGeometryIndex::new(item);
            let remaining = max_matches.saturating_add(1) as usize - matches.len();
            let item_matches = find_matches_in_text_bounded(
                text,
                query,
                case_sensitive,
                whole_word,
                remaining,
                &source_projection,
            )?;

            for item_match in item_matches {
                if matches.len() >= max_matches as usize {
                    truncated = true;
                    break;
                }

                let matched_text = text[item_match.source_start..item_match.source_end].to_string();
                let snippet = build_snippet(
                    text,
                    &source_projection,
                    item_match.start_utf16,
                    item_match.end_utf16,
                    context_chars as usize,
                )?;
                let (bounding_box, bounding_box_level) =
                    geometry_index.match_bounding_box(item_match.start_utf16, item_match.end_utf16);
                matches.push(TextSearchMatch {
                    id: format!("p{page}-match-{}", matches.len() + 1),
                    page,
                    text: matched_text,
                    snippet,
                    match_start: item_match.start_utf16,
                    match_end: item_match.end_utf16,
                    text_item_index: text_item_index as u32,
                    bounding_box,
                    bounding_box_level,
                    route: TEXT_INDEX_ROUTE.into(),
                });
            }

            if truncated {
                break;
            }
        }

        if truncated {
            break;
        }
    }

    Ok(TextSearchResult {
        num_pages,
        searched_pages,
        total_matches: matches.len() as u32,
        matches,
        route: TEXT_INDEX_ROUTE.into(),
        truncated,
        page_cache,
    })
}

pub(super) struct TextGeometryIndex<'a> {
    pub(super) item: &'a PositionedTextItem,
    pub(super) eligible: Vec<usize>,
    #[cfg(test)]
    pub(super) candidate_visits: std::cell::Cell<usize>,
}

impl<'a> TextGeometryIndex<'a> {
    pub(super) fn new(item: &'a PositionedTextItem) -> Self {
        // Extraction emits monotonic half-open UTF-16 ranges. Exclude reversed
        // internal geometry fail-closed instead of making every match pay for
        // an unbounded two-dimensional malformed-range scan.
        let mut eligible = item
            .chars
            .iter()
            .enumerate()
            .filter(|(_, character)| {
                !character.is_whitespace
                    && character.item_char_end >= character.item_char_start
                    && character.bounding_box.is_some()
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if eligible
            .windows(2)
            .any(|pair| item.chars[pair[0]].item_char_start > item.chars[pair[1]].item_char_start)
        {
            eligible.sort_unstable_by_key(|index| {
                let character = &item.chars[*index];
                (character.item_char_start, character.item_char_end, *index)
            });
        }
        Self {
            item,
            eligible,
            #[cfg(test)]
            candidate_visits: std::cell::Cell::new(0),
        }
    }

    pub(super) fn range_candidates(&self, start_utf16: u32, end_utf16: u32) -> &[usize] {
        let first = self
            .eligible
            .partition_point(|index| self.item.chars[*index].item_char_start < start_utf16);
        let count = self.eligible[first..]
            .partition_point(|index| self.item.chars[*index].item_char_start <= end_utf16);
        &self.eligible[first..first + count]
    }

    pub(super) fn match_bounding_box(
        &self,
        start_utf16: u32,
        end_utf16: u32,
    ) -> (Option<TextBoundingBox>, Option<String>) {
        let char_box = self
            .range_candidates(start_utf16, end_utf16)
            .iter()
            .map(|index| {
                #[cfg(test)]
                self.candidate_visits
                    .set(self.candidate_visits.get().saturating_add(1));
                &self.item.chars[*index]
            })
            .filter(|character| character.item_char_end <= end_utf16)
            .filter_map(|character| character.bounding_box)
            .try_fold(None::<TextBoundingBox>, |current, box_| match current {
                None => Some(Some(box_)),
                Some(current) => current.union(box_).map(Some),
            })
            .flatten();
        if let Some(box_) = char_box {
            (Some(box_), Some("char_estimated".to_string()))
        } else if let Some(box_) = self.item.bounding_box {
            (Some(box_), Some("text_item".to_string()))
        } else {
            (None, None)
        }
    }

    #[cfg(test)]
    pub(super) fn candidate_visits(&self) -> usize {
        self.candidate_visits.get()
    }
}

#[cfg(test)]
pub(super) fn match_bounding_box(
    item: &PositionedTextItem,
    start_utf16: u32,
    end_utf16: u32,
) -> (Option<TextBoundingBox>, Option<String>) {
    TextGeometryIndex::new(item).match_bounding_box(start_utf16, end_utf16)
}

pub(super) fn is_word_char(value: Option<char>) -> bool {
    matches!(value, Some(ch) if ch.is_ascii_alphanumeric() || ch == '_')
}

pub(super) fn is_whole_word_match(text: &str, start: usize, end: usize) -> bool {
    let before = text.get(..start).and_then(|s| s.chars().last());
    let after = text.get(end..).and_then(|s| s.chars().next());
    !is_word_char(before) && !is_word_char(after)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TextMatchRange {
    pub(super) source_start: usize,
    pub(super) source_end: usize,
    pub(super) start_utf16: u32,
    pub(super) end_utf16: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiteralTextMatch {
    pub(crate) text: String,
    pub(crate) snippet: String,
    pub(crate) start_utf16: u32,
    pub(crate) end_utf16: u32,
}

#[derive(Debug)]
pub(super) struct NormalizedUtf16Offsets {
    pub(super) by_byte: Vec<(usize, usize)>,
}

impl NormalizedUtf16Offsets {
    pub(super) fn new(text: &str) -> Self {
        let mut by_byte = Vec::with_capacity(text.chars().count() + 1);
        let mut utf16_offset = 0usize;
        for (byte_offset, character) in text.char_indices() {
            by_byte.push((byte_offset, utf16_offset));
            utf16_offset += character.len_utf16();
        }
        by_byte.push((text.len(), utf16_offset));
        Self { by_byte }
    }

    pub(super) fn utf16_at_byte(&self, byte_offset: usize) -> Option<usize> {
        self.by_byte
            .binary_search_by_key(&byte_offset, |(byte, _)| *byte)
            .ok()
            .map(|index| self.by_byte[index].1)
    }
}

#[derive(Debug)]
pub(super) struct SourceUtf16Projection {
    pub(super) by_utf16: Vec<(usize, usize)>,
    pub(super) utf16_len: usize,
}

impl SourceUtf16Projection {
    pub(super) fn new(text: &str) -> Self {
        let mut by_utf16 = Vec::with_capacity(text.chars().count() + 1);
        let mut utf16_offset = 0usize;
        for (byte_offset, character) in text.char_indices() {
            by_utf16.push((utf16_offset, byte_offset));
            utf16_offset += character.len_utf16();
        }
        by_utf16.push((utf16_offset, text.len()));
        Self {
            by_utf16,
            utf16_len: utf16_offset,
        }
    }

    pub(super) fn byte_at_utf16_clamped(
        &self,
        utf16_offset: usize,
        split_message: &'static str,
    ) -> Result<usize, TextIndexError> {
        let utf16_offset = utf16_offset.min(self.utf16_len);
        self.by_utf16
            .binary_search_by_key(&utf16_offset, |(units, _)| *units)
            .ok()
            .map(|index| self.by_utf16[index].1)
            .ok_or_else(|| TextIndexError::invalid_request(split_message))
    }
}

pub(super) fn normalize_for_search(value: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        value.to_string()
    } else {
        value.chars().flat_map(char::to_lowercase).collect()
    }
}

/// Match in the normalized UTF-16 index space used by TS v3.0.14, then apply
/// those offsets directly to the original text for its `slice` semantics.
/// Boundary tables keep projection bounded and fail closed where JavaScript
/// could create a lone surrogate that Rust strings cannot represent.
#[cfg(test)]
pub(super) fn find_matches_in_text(
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

pub(super) fn find_matches_in_text_bounded(
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

pub(super) fn build_snippet(
    text: &str,
    source_projection: &SourceUtf16Projection,
    start_utf16: u32,
    end_utf16: u32,
    context_chars: usize,
) -> Result<String, TextIndexError> {
    let start_utf16 = start_utf16 as usize;
    let end_utf16 = end_utf16 as usize;
    let snippet_start_utf16 = start_utf16.saturating_sub(context_chars);
    let snippet_end_utf16 = end_utf16
        .saturating_add(context_chars)
        .min(source_projection.utf16_len);
    let snippet_start = source_projection.byte_at_utf16_clamped(
        snippet_start_utf16,
        "UTF-16 snippet context would split an astral character.",
    )?;
    let snippet_end = source_projection.byte_at_utf16_clamped(
        snippet_end_utf16,
        "UTF-16 snippet context would split an astral character.",
    )?;
    let prefix = if snippet_start > 0 { "..." } else { "" };
    let suffix = if snippet_end < text.len() { "..." } else { "" };
    Ok(format!(
        "{prefix}{}{suffix}",
        &text[snippet_start..snippet_end]
    ))
}

/// Provider-neutral bounded literal matching with the UTF-16 offsets and
/// snippet semantics used by TS v3.0.14. OCR fusion reuses this rather than
/// maintaining a second search implementation.
pub(crate) fn search_literal_text_bounded(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    context_chars: usize,
    max_results: usize,
) -> Result<Vec<LiteralTextMatch>, TextIndexError> {
    let source_projection = SourceUtf16Projection::new(text);
    find_matches_in_text_bounded(
        text,
        query,
        case_sensitive,
        whole_word,
        max_results,
        &source_projection,
    )?
    .into_iter()
    .map(|range| {
        Ok(LiteralTextMatch {
            text: text[range.source_start..range.source_end].to_string(),
            snippet: build_snippet(
                text,
                &source_projection,
                range.start_utf16,
                range.end_utf16,
                context_chars,
            )?,
            start_utf16: range.start_utf16,
            end_utf16: range.end_utf16,
        })
    })
    .collect()
}
