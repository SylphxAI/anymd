//! Assemble content-stream runs into baseline rows.

use crate::extract::Glyph;

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
    type Row = (f64, f64, Vec<(f64, f64)>, Vec<Glyph>);
    let mut rows: Vec<Row> = Vec::new();
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
