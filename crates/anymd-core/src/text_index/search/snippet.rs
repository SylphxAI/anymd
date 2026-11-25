use crate::text_index::TextIndexError;

use super::SourceUtf16Projection;

pub(in crate::text_index) fn build_snippet(
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
