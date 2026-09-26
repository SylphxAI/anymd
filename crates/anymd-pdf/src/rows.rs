//! Rows of glyphs and the segments (runs of words) inside them.

use std::collections::HashMap;

use crate::extract::{Glyph, RawPage};
use crate::reading::median;

/// A horizontal run of text on one baseline with no large gap inside it.
#[derive(Debug, Clone)]
pub(crate) struct Segment {
    pub(crate) x0: f64,
    pub(crate) x1: f64,
    pub(crate) base: f64,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
    pub(crate) size: f64,
    pub(crate) text: String,
    /// Some(true) when glyph advances are uniform (a monospace font).
    pub(crate) mono: Option<bool>,
    /// The words of `text` (split at inferred spaces) with their extents.
    pub(crate) words: Vec<Word>,
    /// A stand-in for the ruled table with this index, placed in reading
    /// order like text.
    pub(crate) table: Option<usize>,
}

/// One word of a segment and its horizontal extent.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Word {
    pub(crate) x0: f64,
    pub(crate) x1: f64,
    pub(crate) text: String,
}

impl Segment {
    pub(crate) fn chars(&self) -> usize {
        self.text.chars().count()
    }
}

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

/// One glyph for [`infer_word_spaces`]: its extent along the text direction
/// (start and advance end), its font size, and its text.
#[derive(Debug, Clone, Copy)]
pub struct SpacingGlyph<'a> {
    pub x0: f64,
    pub x1: f64,
    pub size: f64,
    pub text: &'a str,
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

/// Group glyphs into rows by baseline (tolerating super/subscripts).
///
/// Glyphs are first chained into runs in content-stream order, and a run only
/// joins a row when it does not overlap the row's runs horizontally, so two
/// nearby lines of different sizes never interleave letter by letter.
pub(crate) fn rows_of(glyphs: Vec<Glyph>) -> Vec<Vec<Glyph>> {
    struct Run {
        base: f64,
        size: f64,
        x0: f64,
        x1: f64,
        glyphs: Vec<Glyph>,
    }
    let mut runs: Vec<Run> = Vec::new();
    for glyph in glyphs {
        if let Some(run) = runs.last_mut() {
            let same_line = (run.base - glyph.base).abs() <= run.size.max(glyph.size) * 0.1;
            let continues =
                glyph.x0 >= run.x1 - run.size * 0.5 && glyph.x0 <= run.x1 + run.size * 1.5;
            if same_line && continues {
                run.x1 = run.x1.max(glyph.x1);
                if !glyph.space {
                    run.size = run.size.max(glyph.size);
                }
                run.glyphs.push(glyph);
                continue;
            }
        }
        runs.push(Run {
            base: glyph.base,
            size: glyph.size,
            x0: glyph.x0,
            x1: glyph.x1,
            glyphs: vec![glyph],
        });
    }
    runs.sort_by(|a, b| b.base.total_cmp(&a.base).then(a.x0.total_cmp(&b.x0)));
    // (reference baseline, reference size, spans, glyphs)
    let mut rows: Vec<(f64, f64, Vec<(f64, f64)>, Vec<Glyph>)> = Vec::new();
    for run in runs {
        let only_space = run.glyphs.iter().all(|g| g.space);
        if let Some((ref_base, ref_size, spans, row)) = rows.last_mut() {
            let big = ref_size.max(run.size);
            let small = ref_size.min(run.size);
            let tolerance = if small < big * 0.85 {
                0.6 * big
            } else {
                0.45 * big
            };
            let near = (*ref_base - run.base).abs() <= tolerance;
            let overlaps = spans.iter().any(|&(a, b)| {
                let overlap = b.min(run.x1) - a.max(run.x0);
                overlap > (b - a).min(run.x1 - run.x0).max(0.0) * 0.3 && overlap > small * 0.3
            });
            if near && (!overlaps || only_space) {
                if run.size > *ref_size * 1.05 && !only_space {
                    *ref_base = run.base;
                    *ref_size = run.size;
                }
                spans.push((run.x0, run.x1));
                row.extend(run.glyphs);
                continue;
            }
        }
        rows.push((run.base, run.size, vec![(run.x0, run.x1)], run.glyphs));
    }
    rows.into_iter().map(|(_, _, _, row)| row).collect()
}

/// Split a row into segments at large gaps, inferring word spaces.
pub(crate) fn segments_of_row(mut row: Vec<Glyph>) -> Vec<Segment> {
    row.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let dominant = dominant_size(row.iter().filter(|g| !g.space).map(|g| (g.size, 1)));
    let mut bases: Vec<f64> = row
        .iter()
        .filter(|g| !g.space && g.size >= dominant * 0.95)
        .map(|g| g.base)
        .collect();
    let ref_base = median(&mut bases);

    // Pass 1: drop overprinted duplicates and split into groups at large gaps.
    // Each entry is (glyph, explicit space before it).
    let mut groups: Vec<Vec<(Glyph, bool)>> = Vec::new();
    let mut end = f64::NEG_INFINITY;
    let mut pending_space = false;
    for glyph in row {
        if glyph.space {
            pending_space = true;
            if end.is_finite() {
                end = end.max(glyph.x1.min(glyph.x0 + glyph.size * 0.6));
            }
            continue;
        }
        if let Some((last, _)) = groups.last().and_then(|group| group.last()) {
            if last.text == glyph.text && (glyph.x0 - last.x0).abs() < glyph.size * 0.12 {
                continue;
            }
        }
        let size_ref = glyph.size.max(dominant * 0.8);
        let gap = glyph.x0 - end;
        if groups.is_empty() || gap > size_ref * 1.2 {
            groups.push(Vec::new());
            pending_space = false;
        }
        end = if gap > size_ref * 1.2 {
            glyph.x1
        } else {
            end.max(glyph.x1)
        };
        groups
            .last_mut()
            .expect("group")
            .push((glyph, std::mem::take(&mut pending_space)));
    }

    // Pass 2: text per group, with a space threshold that adapts to tracking.
    let mut segments = Vec::with_capacity(groups.len());
    for group in groups {
        let mut gaps: Vec<f64> = Vec::with_capacity(group.len());
        let mut reach = f64::NEG_INFINITY;
        for (glyph, _) in &group {
            if reach.is_finite() {
                gaps.push((glyph.x0 - reach) / glyph.size.max(0.1));
            }
            reach = reach.max(glyph.x1);
        }
        let (letter_gap, base_threshold) = space_thresholds(&gaps);
        let group_mono = monospace(&group);
        // Super- and subscripts are judged against their own group, not the
        // whole row: a smaller table beside a body-text column is not a
        // subscript of that column.
        let dominant = {
            let local = dominant_size(group.iter().map(|(g, _)| (g.size, 1)));
            if local > 0.0 {
                local
            } else {
                dominant
            }
        };
        let ref_base = {
            let mut bases: Vec<f64> = group
                .iter()
                .filter(|(g, _)| g.size >= dominant * 0.95)
                .map(|(g, _)| g.base)
                .collect();
            if bases.is_empty() {
                ref_base
            } else {
                median(&mut bases)
            }
        };
        let mut segment: Option<Segment> = None;
        let mut script = 0i8;
        let mut reach = f64::NEG_INFINITY;
        for (glyph, explicit_space) in group {
            let Some(current) = segment.as_mut() else {
                reach = glyph.x1;
                segment = Some(new_segment(&glyph, dominant));
                continue;
            };
            let gap = glyph.x0 - reach;
            let prev_char = current.text.chars().last();
            let next_char = glyph.text.chars().next();
            let wants_space = wants_word_space(
                prev_char,
                next_char,
                gap,
                space_scale(current.size, glyph.size),
                explicit_space && letter_gap <= 0.12,
                base_threshold,
            );
            if wants_space && !current.text.ends_with(' ') {
                current.text.push(' ');
                current.words.push(Word {
                    x0: glyph.x0,
                    x1: glyph.x1,
                    text: String::new(),
                });
            }
            let glyph_script = if dominant <= 0.0 || glyph.size > dominant * 0.85 {
                0
            } else if glyph.base - ref_base > dominant * 0.2 {
                1
            } else if glyph.base - ref_base < -dominant * 0.12 {
                -1
            } else {
                0
            };
            let word = current.words.last_mut().expect("a segment has a word");
            if glyph_script != script {
                let marks = next_char.is_some_and(char::is_alphanumeric);
                if glyph_script != 0 && marks && !wants_space && prev_char.is_some() {
                    let mark = if glyph_script > 0 { '^' } else { '_' };
                    current.text.push(mark);
                    word.text.push(mark);
                    script = glyph_script;
                } else if glyph_script == 0 || !marks {
                    script = 0;
                }
            }
            current.text.push_str(&glyph.text);
            word.text.push_str(&glyph.text);
            word.x1 = word.x1.max(glyph.x1);
            current.x1 = current.x1.max(glyph.x1);
            current.top = current.top.max(glyph.base + glyph.size * 0.8);
            current.bottom = current.bottom.min(glyph.base - glyph.size * 0.2);
            reach = reach.max(glyph.x1);
        }
        if let Some(mut segment) = segment {
            segment.mono = group_mono;
            let trimmed = segment.text.trim();
            if trimmed.len() != segment.text.len() {
                segment.text = trimmed.to_string();
            }
            segment.words.retain(|word| !word.text.trim().is_empty());
            if !segment.text.is_empty() {
                segments.push(segment);
            }
        }
    }
    segments
}

pub(crate) fn new_segment(glyph: &Glyph, dominant: f64) -> Segment {
    Segment {
        x0: glyph.x0,
        x1: glyph.x1,
        base: glyph.base,
        top: glyph.base + glyph.size * 0.8,
        bottom: glyph.base - glyph.size * 0.2,
        size: if dominant > 0.0 { dominant } else { glyph.size },
        text: glyph.text.clone(),
        mono: None,
        words: vec![Word {
            x0: glyph.x0,
            x1: glyph.x1,
            text: glyph.text.clone(),
        }],
        table: None,
    }
}

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

/// The text of a row: its segments joined by spaces.
pub(crate) fn row_text(row: &[Segment]) -> String {
    let mut text = String::new();
    for segment in row {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&segment.text);
    }
    text
}
