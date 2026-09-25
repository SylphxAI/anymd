//! Semantic role hints (heading, caption, list, header/footer) for text elements.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use super::{pattern, PageText};
use crate::text_index::TextBoundingBox;

static CAPTION_PREFIX_PATTERN: OnceLock<Regex> = OnceLock::new();
static FOOTER_PATTERN: OnceLock<Regex> = OnceLock::new();
static HEADER_PATTERN: OnceLock<Regex> = OnceLock::new();
static LIST_PREFIX_PATTERN: OnceLock<Regex> = OnceLock::new();
static NUMBERED_SECTION_PATTERN: OnceLock<Regex> = OnceLock::new();
static ROMAN_SECTION_PATTERN: OnceLock<Regex> = OnceLock::new();
static NAMED_SECTION_PATTERN: OnceLock<Regex> = OnceLock::new();

/// Caption prefix ("Figure 3:", "Table II.", ...), shared by semantic hints and
/// Document AST caption linking.
pub(super) fn caption_prefix_pattern() -> &'static Regex {
    pattern(
        &CAPTION_PREFIX_PATTERN,
        r"(?iu)^(fig(?:ure)?|table|chart|graph|plot|formula|eq(?:uation)?|image|diagram|algorithm|exhibit)\.?(?:(?:\s*(?:\(?[a-z]?\d+(?:[.-]\d+)*[a-z]?\)?|\([A-Z]\)|[ivxlcdm]+)(?:\s*[:.)–—-]|\s+|$))|\s*[:)–—-])",
    )
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct SemanticHint {
    role: &'static str,
    confidence: f64,
    signals: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PageSemanticBounds {
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PageSemanticStats {
    max_height: f64,
    median_height: f64,
    text_item_count: usize,
    bounds: Option<PageSemanticBounds>,
}

pub(super) fn page_semantic_bounds(
    page_geometry: Option<&Value>,
) -> BTreeMap<u32, PageSemanticBounds> {
    let mut bounds = BTreeMap::new();
    for geometry in page_geometry
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(page) = geometry.get("page").and_then(Value::as_u64) else {
            continue;
        };
        let Some(view_box) = geometry.get("view_box") else {
            continue;
        };
        let Some(left) = view_box.get("left").and_then(Value::as_f64) else {
            continue;
        };
        let Some(right) = view_box.get("right").and_then(Value::as_f64) else {
            continue;
        };
        let Some(bottom) = view_box.get("bottom").and_then(Value::as_f64) else {
            continue;
        };
        let Some(top) = view_box.get("top").and_then(Value::as_f64) else {
            continue;
        };
        if page > u64::from(u32::MAX)
            || ![left, right, bottom, top]
                .iter()
                .all(|value| value.is_finite())
            || right <= left
            || top <= bottom
        {
            continue;
        }
        bounds.insert(
            page as u32,
            PageSemanticBounds {
                left,
                right,
                bottom,
                top,
            },
        );
    }
    bounds
}

pub(super) fn page_semantic_stats(
    page: &PageText,
    bounds: Option<PageSemanticBounds>,
) -> PageSemanticStats {
    let mut heights = page
        .positioned_items
        .iter()
        .filter(|item| !item.text.trim().is_empty())
        .filter_map(|item| item.bounding_box.map(|box_| box_.top - box_.bottom))
        .filter(|height| height.is_finite() && *height != 0.0)
        .collect::<Vec<_>>();
    heights.sort_by(f64::total_cmp);
    let midpoint = heights.len() / 2;
    let median_height = if heights.is_empty() {
        0.0
    } else if heights.len().is_multiple_of(2) {
        (heights[midpoint - 1] + heights[midpoint]) / 2.0
    } else {
        heights[midpoint]
    };
    PageSemanticStats {
        max_height: heights.last().copied().unwrap_or(0.0),
        median_height,
        text_item_count: heights.len(),
        bounds,
    }
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

fn compact_text_height(height: f64, stats: PageSemanticStats) -> bool {
    if height <= 0.0 {
        return false;
    }
    if stats.median_height <= 0.0 {
        return height <= 12.0;
    }
    height <= (stats.median_height * 1.25).max(stats.median_height + 2.0)
}

pub(super) fn semantic_hint(
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

    if let (Some(box_), Some(bounds)) = (bounding_box, stats.bounds) {
        let page_height = bounds.top - bounds.bottom;
        let edge_zone = 36.0_f64.max(page_height * 0.08);
        let near_top = box_.top >= bounds.top - edge_zone;
        let near_bottom = box_.bottom <= bounds.bottom + edge_zone;
        let within_horizontal = box_.left >= bounds.left - 4.0 && box_.right <= bounds.right + 4.0;
        let height = box_.top - box_.bottom;
        let compact = compact_text_height(height, stats);
        let short_line = value.encode_utf16().count() <= 140;
        let footer = pattern(
            &FOOTER_PATTERN,
            r"(?iu)^(?:page\s*)?\d+\s*(?:/|of)\s*\d+$|^page\s+\d+$|copyright|all rights reserved",
        )
        .is_match(value);
        if near_bottom && within_horizontal && short_line && footer {
            let mut signals = vec!["page-bottom-band"];
            if compact {
                signals.push("compact-edge-text");
            }
            signals.push("footer-pattern");
            return SemanticHint {
                role: "footer",
                confidence: 0.88,
                signals,
                level: None,
            };
        }
        if near_top
            && within_horizontal
            && short_line
            && compact
            && pattern(
                &HEADER_PATTERN,
                r"(?iu)\b(?:confidential|draft|internal|prepared\s+(?:for|by))\b",
            )
            .is_match(value)
        {
            return SemanticHint {
                role: "header",
                confidence: 0.82,
                signals: vec!["page-top-band", "compact-edge-text", "header-pattern"],
                level: None,
            };
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_index::PositionedTextItem;
    use serde_json::json;

    #[test]
    fn semantic_hints_match_ts_precedence_and_complete_metadata() {
        let bounds = PageSemanticBounds {
            left: 0.0,
            right: 612.0,
            bottom: 0.0,
            top: 792.0,
        };
        let stats = PageSemanticStats {
            max_height: 18.0,
            median_height: 10.0,
            text_item_count: 8,
            bounds: Some(bounds),
        };
        let box_at = |left, bottom, right, top| TextBoundingBox {
            left,
            bottom,
            right,
            top,
        };
        let value = |text, box_| serde_json::to_value(semantic_hint(text, box_, stats)).unwrap();

        assert_eq!(
            value(
                "Equation (1): Loss function",
                Some(box_at(10.0, 770.0, 220.0, 780.0))
            ),
            json!({"role":"caption","confidence":0.86,"signals":["caption-prefix"]})
        );
        assert_eq!(
            value(
                "Confidential Report",
                Some(box_at(10.0, 770.0, 160.0, 780.0))
            ),
            json!({"role":"header","confidence":0.82,"signals":["page-top-band","compact-edge-text","header-pattern"]})
        );
        assert_eq!(
            value("Page 1 of 2", Some(box_at(10.0, 10.0, 90.0, 20.0))),
            json!({"role":"footer","confidence":0.88,"signals":["page-bottom-band","compact-edge-text","footer-pattern"]})
        );
        assert_eq!(
            value("Chapter 1: Intro", Some(box_at(10.0, 500.0, 150.0, 512.0))),
            json!({"role":"heading","confidence":0.84,"signals":["section-heading-pattern","named-section-prefix","short-line"],"level":1})
        );
        assert_eq!(
            value("1.2 Scope", Some(box_at(10.0, 480.0, 100.0, 492.0))),
            json!({"role":"heading","confidence":0.84,"signals":["section-heading-pattern","numbered-section-prefix","short-line"],"level":2})
        );
        assert_eq!(
            value("IV. Results", Some(box_at(10.0, 460.0, 100.0, 472.0))),
            json!({"role":"heading","confidence":0.82,"signals":["section-heading-pattern","roman-section-prefix","short-line"],"level":1})
        );
        assert_eq!(
            value("[x] item", None),
            json!({"role":"list_item","confidence":0.92,"signals":["list-prefix"]})
        );
        assert_eq!(
            value("Ordinary sentence.", None),
            json!({"role":"paragraph","confidence":0.5,"signals":["default-text"]})
        );
    }

    #[test]
    fn semantic_hints_use_utf16_thresholds_and_valid_page_bounds() {
        let stats = PageSemanticStats {
            max_height: 10.0,
            median_height: 10.0,
            text_item_count: 2,
            bounds: None,
        };
        let title_120 = format!("A{}😀", "x".repeat(117));
        let title_121 = format!("A{}😀", "x".repeat(118));
        assert_eq!(title_120.encode_utf16().count(), 120);
        assert_eq!(title_121.encode_utf16().count(), 121);
        assert_eq!(
            semantic_hint(&format!("1 {title_120}"), None, stats).role,
            "heading"
        );
        assert_eq!(
            semantic_hint(&format!("1 {title_121}"), None, stats).role,
            "paragraph"
        );

        let geometry = json!([
            {"page":1,"view_box":{"left":0,"bottom":0,"right":612,"top":792}},
            {"page":2,"view_box":{"left":0,"bottom":0,"right":0,"top":792}},
            {"page":3,"view_box":{"left":0,"bottom":0,"right":"bad","top":792}}
        ]);
        let parsed = page_semantic_bounds(Some(&geometry));
        assert!(parsed.contains_key(&1));
        assert!(!parsed.contains_key(&2));
        assert!(!parsed.contains_key(&3));

        let edge_box = TextBoundingBox {
            left: -20.0,
            bottom: 770.0,
            right: 100.0,
            top: 780.0,
        };
        let with_bounds = PageSemanticStats {
            bounds: parsed.get(&1).copied(),
            ..stats
        };
        assert_eq!(
            semantic_hint("Confidential Report", Some(edge_box), with_bounds).role,
            "paragraph"
        );
    }

    #[test]
    fn semantic_height_stats_handle_even_odd_and_admitted_maximum_linearly() {
        let make_item = |height: f64| PositionedTextItem {
            text: "x".into(),
            bounding_box: Some(TextBoundingBox {
                left: 0.0,
                bottom: 0.0,
                right: 1.0,
                top: height,
            }),
            chars: Vec::new(),
            runs: Vec::new(),
        };
        let even = PageText {
            page: 1,
            text: String::new(),
            positioned_items: vec![make_item(8.0), make_item(12.0)],
        };
        assert_eq!(page_semantic_stats(&even, None).median_height, 10.0);
        let odd = PageText {
            page: 1,
            text: String::new(),
            positioned_items: vec![make_item(8.0), make_item(12.0), make_item(18.0)],
        };
        assert_eq!(page_semantic_stats(&odd, None).median_height, 12.0);

        let admitted = PageText {
            page: u32::MAX,
            text: String::new(),
            positioned_items: (0..250_000).map(|_| make_item(10.0)).collect(),
        };
        let admitted_stats = page_semantic_stats(&admitted, None);
        assert_eq!(admitted_stats.text_item_count, 250_000);
        assert_eq!(admitted_stats.median_height, 10.0);
        assert_eq!(admitted_stats.max_height, 10.0);
    }
}
