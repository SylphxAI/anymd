//! Infer font metrics from page glyphs and text runs.

use std::collections::HashMap;

use super::is_cjk;
use crate::extract::{Glyph, RawPage};

/// Monospace test: every distinct character has the same advance width.
/// Proportional fonts spread widely once a few distinct glyphs are present.
pub(crate) fn monospace(group: &[(Glyph, bool)]) -> Option<bool> {
    // Full-width CJK is uniform by design; it is not a monospace hint.
    let cjk = group
        .iter()
        .filter(|(g, _)| g.text.chars().next().is_some_and(is_cjk))
        .count();
    if cjk * 4 >= group.len() {
        return None;
    }
    let mut seen: HashMap<&str, f64> = HashMap::new();
    for (glyph, _) in group {
        let advance = (glyph.x1 - glyph.x0) / glyph.size.max(0.1);
        seen.entry(glyph.text.as_str()).or_insert(advance);
    }
    if seen.len() < 5 {
        return None;
    }
    let max = seen.values().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min = seen.values().cloned().fold(f64::INFINITY, f64::min);
    Some(max > 0.0 && (max - min) / max < 0.08)
}

pub(crate) fn dominant_size(sizes: impl Iterator<Item = (f64, usize)>) -> f64 {
    let mut buckets: HashMap<i64, (usize, f64)> = HashMap::new();
    for (size, weight) in sizes {
        let entry = buckets
            .entry((size * 2.0).round() as i64)
            .or_insert((0, size));
        entry.0 += weight;
    }
    buckets
        .into_values()
        .max_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
        .map(|(_, size)| size)
        .unwrap_or(0.0)
}

pub(crate) fn body_font_size(pages: &[RawPage]) -> f64 {
    let size = dominant_size(
        pages
            .iter()
            .filter_map(|page| page.glyphs.as_ref().ok())
            .flatten()
            .filter(|glyph| !glyph.space)
            .map(|glyph| (glyph.size, 1)),
    );
    if size > 0.0 {
        size
    } else {
        10.0
    }
}
