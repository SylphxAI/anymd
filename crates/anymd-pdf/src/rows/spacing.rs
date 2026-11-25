//! Infer word boundaries from glyph geometry and script.

use super::SpacingGlyph;

/// Letter gap (tracking) and word-space threshold, both in units of font
/// size, from the normalized gaps between consecutive glyphs of one run.
///
/// Tracked text (letter-spaced headings) raises the threshold above its own
/// letter gap so every letter does not become a word.
pub(crate) fn space_thresholds(gaps: &[f64]) -> (f64, f64) {
    let letter_gap = if gaps.len() >= 4 {
        let mut sorted = gaps.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted[sorted.len() / 4].max(0.0)
    } else {
        0.0
    };
    let base_threshold = if letter_gap > 0.12 {
        letter_gap + (letter_gap * 0.8).max(0.15)
    } else {
        0.16
    };
    (letter_gap, base_threshold)
}

/// The size a gap is measured against: the smaller of two neighbours, but
/// never below 70% of the previous one, so a superscript does not shrink it.
pub(crate) fn space_scale(previous_size: f64, size: f64) -> f64 {
    size.max(previous_size * 0.7).min(previous_size.max(size))
}

/// Whether a word space belongs between `prev` and `next` given their gap.
/// CJK text is set without spaces, so only a wide gap separates two CJK glyphs.
pub(crate) fn wants_word_space(
    prev: Option<char>,
    next: Option<char>,
    gap: f64,
    scale: f64,
    explicit_space: bool,
    base_threshold: f64,
) -> bool {
    if prev.is_some_and(is_cjk) && next.is_some_and(is_cjk) {
        gap > 0.5 * scale
    } else {
        explicit_space || gap > base_threshold * scale
    }
}

/// Word-space inference of the layout engine for callers with their own
/// glyph stream: for glyphs of one line in reading order, `true` at index `i`
/// means a word space belongs before glyph `i`.
///
/// Uses the same rules as the Markdown path: a gap wider than 0.16 of the
/// font size, a threshold that adapts to letter-spaced text, and no spaces
/// between CJK glyphs. Whitespace glyphs are already spaces, so they and the
/// glyph after them never get one; glyphs with non-finite geometry never do.
pub fn infer_word_spaces(glyphs: &[SpacingGlyph<'_>]) -> Vec<bool> {
    let usable = |glyph: &SpacingGlyph<'_>| {
        glyph.x0.is_finite()
            && glyph.x1.is_finite()
            && glyph.size.is_finite()
            && glyph.size > 0.0
            && !glyph.text.is_empty()
            && !glyph.text.chars().all(char::is_whitespace)
    };
    let mut gaps = Vec::with_capacity(glyphs.len());
    let mut reach = f64::NEG_INFINITY;
    for glyph in glyphs.iter().filter(|glyph| usable(glyph)) {
        if reach.is_finite() {
            gaps.push((glyph.x0 - reach) / glyph.size.max(0.1));
        }
        reach = reach.max(glyph.x1);
    }
    let (_, base_threshold) = space_thresholds(&gaps);

    let mut flags = vec![false; glyphs.len()];
    let mut previous: Option<&SpacingGlyph<'_>> = None;
    let mut after_space = false;
    let mut reach = f64::NEG_INFINITY;
    for (index, glyph) in glyphs.iter().enumerate() {
        if !usable(glyph) {
            if glyph.text.chars().all(char::is_whitespace) {
                after_space = true;
            } else {
                // Unknown geometry: never guess a space across it.
                previous = None;
                reach = f64::NEG_INFINITY;
            }
            continue;
        }
        if let Some(prev) = previous {
            flags[index] = !after_space
                && wants_word_space(
                    prev.text.chars().last(),
                    glyph.text.chars().next(),
                    glyph.x0 - reach,
                    space_scale(prev.size, glyph.size),
                    false,
                    base_threshold,
                );
        }
        after_space = false;
        previous = Some(glyph);
        reach = reach.max(glyph.x1);
    }
    flags
}

pub(crate) fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x3000..=0x30FF | 0x3100..=0x31FF |
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF |
        0xFF00..=0xFFEF | 0x20000..=0x2FA1F)
}
