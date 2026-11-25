//! Header and footer classification within page edge bands.

use std::sync::OnceLock;

use regex::Regex;

use super::{PageSemanticStats, SemanticHint};
use crate::document_twin::pattern;
use crate::text_index::TextBoundingBox;

static FOOTER_PATTERN: OnceLock<Regex> = OnceLock::new();
static HEADER_PATTERN: OnceLock<Regex> = OnceLock::new();

fn compact_text_height(height: f64, stats: PageSemanticStats) -> bool {
    if height <= 0.0 {
        return false;
    }
    if stats.median_height <= 0.0 {
        return height <= 12.0;
    }
    height <= (stats.median_height * 1.25).max(stats.median_height + 2.0)
}

pub(super) fn running_text_hint(
    value: &str,
    bounding_box: Option<TextBoundingBox>,
    stats: PageSemanticStats,
) -> Option<SemanticHint> {
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
            return Some(SemanticHint {
                role: "footer",
                confidence: 0.88,
                signals,
                level: None,
            });
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
            return Some(SemanticHint {
                role: "header",
                confidence: 0.82,
                signals: vec!["page-top-band", "compact-edge-text", "header-pattern"],
                level: None,
            });
        }
    }
    None
}
