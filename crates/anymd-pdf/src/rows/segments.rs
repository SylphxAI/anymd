//! Split each baseline row into text runs and assemble their words.

use super::{
    dominant_size, monospace, new_segment, space_scale, space_thresholds, wants_word_space,
    Segment, Word,
};
use crate::extract::Glyph;
use crate::reading::median;

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
