use std::path::Path;

use crate::text_index::{
    extract_pdf_text, TextIndexError, TextSearchMatch, TextSearchResult, TEXT_INDEX_ROUTE,
};

use super::{
    build_snippet, find_matches_in_text_bounded, LiteralTextMatch, SourceUtf16Projection,
    TextGeometryIndex,
};

pub(super) struct PdfSearchOptions {
    pub(super) case_sensitive: bool,
    pub(super) whole_word: bool,
    pub(super) max_pages: u32,
    pub(super) max_matches: u32,
    pub(super) context_chars: u32,
}

pub(super) fn search_pdf(
    path: &Path,
    max_file_bytes: u64,
    query: &str,
    requested_pages: Option<&[u32]>,
    options: PdfSearchOptions,
) -> Result<TextSearchResult, TextIndexError> {
    let PdfSearchOptions {
        case_sensitive,
        whole_word,
        max_pages,
        max_matches,
        context_chars,
    } = options;
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
