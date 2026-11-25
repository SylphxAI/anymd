//! Role classification in precedence order: caption, running text, section,
//! list, large-text heading, then paragraph.

use std::sync::OnceLock;

use regex::Regex;

use super::{running_text::running_text_hint, PageSemanticStats, SemanticHint};
use crate::document_twin::pattern;
use crate::text_index::TextBoundingBox;

static CAPTION_PREFIX_PATTERN: OnceLock<Regex> = OnceLock::new();
static LIST_PREFIX_PATTERN: OnceLock<Regex> = OnceLock::new();
static NUMBERED_SECTION_PATTERN: OnceLock<Regex> = OnceLock::new();
static ROMAN_SECTION_PATTERN: OnceLock<Regex> = OnceLock::new();
static NAMED_SECTION_PATTERN: OnceLock<Regex> = OnceLock::new();

/// Caption prefix ("Figure 3:", "Table II.", ...), shared by semantic hints and
/// Document AST caption linking.
pub(in crate::document_twin) fn caption_prefix_pattern() -> &'static Regex {
    pattern(
        &CAPTION_PREFIX_PATTERN,
        r"(?iu)^(fig(?:ure)?|table|chart|graph|plot|formula|eq(?:uation)?|image|diagram|algorithm|exhibit)\.?(?:(?:\s*(?:\(?[a-z]?\d+(?:[.-]\d+)*[a-z]?\)?|\([A-Z]\)|[ivxlcdm]+)(?:\s*[:.)–—-]|\s+|$))|\s*[:)–—-])",
    )
}

fn title_like_heading_text(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.encode_utf16().count() <= 120
        && !value.ends_with(['.', '!', '?'])
        && value
            .chars()
            .next()
            .is_some_and(|first| first.is_uppercase() || first.is_ascii_digit())
}

pub(in crate::document_twin) fn semantic_hint(
    value: &str,
    bounding_box: Option<TextBoundingBox>,
    stats: PageSemanticStats,
) -> SemanticHint {
    let value = value.trim();
    if caption_prefix_pattern().is_match(value) {
        return SemanticHint {
            role: "caption",
            confidence: 0.86,
            signals: vec!["caption-prefix"],
            level: None,
        };
    }

    if let Some(hint) = running_text_hint(value, bounding_box, stats) {
        return hint;
    }

    if let Some(captures) = pattern(
        &NAMED_SECTION_PATTERN,
        r"(?iu)^(appendix|chapter|section|part)\s+([A-Z0-9]+(?:\.\d+)*)(?:\s*[:.–—-]|\s+)\s*(.+)$",
    )
    .captures(value)
    {
        if captures
            .get(3)
            .is_some_and(|title| title_like_heading_text(title.as_str()))
        {
            return SemanticHint {
                role: "heading",
                confidence: 0.84,
                signals: vec![
                    "section-heading-pattern",
                    "named-section-prefix",
                    "short-line",
                ],
                level: Some(1),
            };
        }
    }
    if let Some(captures) = pattern(
        &NUMBERED_SECTION_PATTERN,
        r"^(\d+(?:\.\d+)*)(?:\.)?\s+(.+)$",
    )
    .captures(value)
    {
        if captures
            .get(2)
            .is_some_and(|title| title_like_heading_text(title.as_str()))
        {
            let level = captures
                .get(1)
                .map(|label| {
                    label
                        .as_str()
                        .split('.')
                        .filter(|part| !part.is_empty())
                        .count()
                })
                .unwrap_or(1)
                .clamp(1, 6) as u8;
            return SemanticHint {
                role: "heading",
                confidence: 0.84,
                signals: vec![
                    "section-heading-pattern",
                    "numbered-section-prefix",
                    "short-line",
                ],
                level: Some(level),
            };
        }
    }
    if let Some(captures) =
        pattern(&ROMAN_SECTION_PATTERN, r"^([IVXLCDM]+)\.\s+(.+)$").captures(value)
    {
        if captures
            .get(2)
            .is_some_and(|title| title_like_heading_text(title.as_str()))
        {
            return SemanticHint {
                role: "heading",
                confidence: 0.82,
                signals: vec![
                    "section-heading-pattern",
                    "roman-section-prefix",
                    "short-line",
                ],
                level: Some(1),
            };
        }
    }
    if pattern(
        &LIST_PREFIX_PATTERN,
        r"(?iu)^(?:[-*•◦▪▫–—]\s+|(?:\[[ xX]\]|☐|☑)\s+|(?:\d+|[a-z]|[ivxlcdm]+)[.)]\s+)",
    )
    .is_match(value)
    {
        return SemanticHint {
            role: "list_item",
            confidence: 0.92,
            signals: vec!["list-prefix"],
            level: None,
        };
    }

    let height = bounding_box.map_or(0.0, |box_| box_.top - box_.bottom);
    let short_line = value.encode_utf16().count() <= 120;
    let ends_like_sentence = value.ends_with(['.', '!', '?']);
    let large_text = stats.text_item_count > 1
        && height > 0.0
        && stats.median_height > 0.0
        && height >= stats.median_height * 1.3
        && height >= stats.max_height * 0.8;
    if large_text && short_line && !ends_like_sentence {
        let ratio = height / stats.median_height;
        let level = if ratio >= 1.8 {
            1
        } else if ratio >= 1.55 {
            2
        } else {
            3
        };
        return SemanticHint {
            role: "heading",
            confidence: 0.78,
            signals: vec!["larger-text", "short-line"],
            level: Some(level),
        };
    }

    SemanticHint {
        role: "paragraph",
        confidence: 0.5,
        signals: vec!["default-text"],
        level: None,
    }
}
