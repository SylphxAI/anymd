//! Literal text search over extracted PDF pages: matching, whole-word checks, snippets and match geometry.

use std::path::Path;

use super::{TextIndexError, TextSearchResult};

mod execute;
mod geometry;
mod matching;
mod projection;
mod snippet;

pub(crate) use execute::search_literal_text_bounded;
#[cfg(test)]
pub(super) use geometry::match_bounding_box;
pub(super) use geometry::TextGeometryIndex;
pub(crate) use matching::LiteralTextMatch;
pub(super) use matching::*;
pub(super) use projection::{normalize_for_search, NormalizedUtf16Offsets, SourceUtf16Projection};
pub(super) use snippet::build_snippet;

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
    execute::search_pdf(
        path,
        max_file_bytes,
        query,
        requested_pages,
        execute::PdfSearchOptions {
            case_sensitive,
            whole_word,
            max_pages,
            max_matches,
            context_chars,
        },
    )
}
