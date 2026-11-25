//! Filters hidden glyphs and preserves invisible OCR text over scanned pages.

use super::collector::{Collector, Ink};
use super::{Glyph, Placement};

impl Collector {
    /// Whether scan-like images (as judged by `scan_like`) cover at least
    /// half of the page. Image boxes are united, so overlaps count once.
    fn covered_by_images(&self, scan_like: &dyn Fn(&Placement) -> bool) -> bool {
        let Some(media) = self.media else {
            return false;
        };
        let area = ((media.urx - media.llx) * (media.ury - media.lly)).abs();
        if !(area.is_finite() && area > 0.0) {
            return false;
        }
        let (px0, px1) = (media.llx.min(media.urx), media.llx.max(media.urx));
        let (py0, py1) = (media.lly.min(media.ury), media.lly.max(media.ury));
        let boxes: Vec<[f64; 4]> = self
            .images
            .iter()
            .filter(|image| image.bbox.iter().all(|v| v.is_finite()) && scan_like(image))
            .map(|image| {
                let b = image.bbox;
                [b[0].max(px0), b[1].max(py0), b[2].min(px1), b[3].min(py1)]
            })
            .filter(|b| b[2] > b[0] && b[3] > b[1])
            .collect();
        let mut xs: Vec<f64> = boxes.iter().flat_map(|b| [b[0], b[2]]).collect();
        xs.sort_by(|a, b| a.total_cmp(b));
        xs.dedup();
        let mut covered = 0.0;
        for strip in xs.windows(2) {
            let mut spans: Vec<(f64, f64)> = boxes
                .iter()
                .filter(|b| b[0] <= strip[0] && b[2] >= strip[1])
                .map(|b| (b[1], b[3]))
                .collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (mut height, mut end) = (0.0, f64::NEG_INFINITY);
            for (lo, hi) in spans {
                let lo = lo.max(end);
                if hi > lo {
                    height += hi - lo;
                }
                end = end.max(hi);
            }
            covered += height * (strip[1] - strip[0]);
        }
        covered >= area * 0.5
    }

    /// Glyphs to read. Text a reader cannot see is dropped (#776): hidden
    /// text is how a PDF smuggles instructions to an agent. That covers
    /// clip-only text (mode 7), text in the same colour as the box it sits on
    /// (a table cell or coloured panel), and invisible text (mode 3), with one
    /// exception: on a scanned page, mode-3 text lying over a scan-like image
    /// is the OCR text layer and is kept with its position. A page is scanned
    /// when it has fewer than `SPARSE_PAGE_CHARS` visible letters and digits
    /// and scan-like images (`scan_like`, judged from the image dictionary)
    /// cover at least half of it; a page with real visible text, or only a
    /// stretched tiny image, admits no invisible text. There the layer is
    /// still dropped where it repeats visible text. Returns the glyphs and
    /// whether any invisible-layer text was kept.
    pub(crate) fn visible_glyphs(
        &mut self,
        scan_like: &dyn Fn(&Placement) -> bool,
    ) -> (Vec<Glyph>, bool) {
        let glyphs = std::mem::take(&mut self.glyphs);
        if self.glyph_paint.len() != glyphs.len() || self.rotated_paint.len() != self.rotated.len()
        {
            return (glyphs, false);
        }
        let same = |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.06);
        // Only a glyph painted in some box's colour can be hidden by one, so
        // the boxes are searched for those glyphs alone.
        let mut fills: Vec<[f64; 3]> = Vec::new();
        for (_, fill, _) in &self.boxes {
            if fill.iter().all(|v| v.is_finite()) && !fills.iter().any(|f| same(f, fill)) {
                fills.push(*fill);
            }
        }
        // Visible glyphs first: their letters decide whether the page is one
        // that has a text layer of its own.
        let mut kept: Vec<bool> = glyphs
            .iter()
            .zip(&self.glyph_paint)
            .map(|(glyph, (color, ink, when))| {
                if *ink != Ink::Seen {
                    return false;
                }
                let Some(color) = color else { return true };
                if !fills.iter().any(|fill| same(fill, color)) {
                    return true;
                }
                let (cx, cy) = ((glyph.x0 + glyph.x1) / 2.0, glyph.base + glyph.size * 0.3);
                let under = self.boxes.iter().rev().find(|(b, _, at)| {
                    at < when && cx >= b[0] && cx <= b[2] && cy >= b[1] && cy <= b[3]
                });
                !under.is_some_and(|(_, fill, _)| same(fill, color))
            })
            .collect();
        let native_alnum = glyphs
            .iter()
            .zip(&kept)
            .filter(|(_, k)| **k)
            .map(|(g, _)| g)
            .chain(
                self.rotated
                    .iter()
                    .zip(&self.rotated_paint)
                    .filter(|(_, (ink, _))| *ink == Ink::Seen)
                    .map(|(g, _)| g),
            )
            .flat_map(|g| g.text.chars())
            .filter(|c| c.is_alphanumeric())
            .count();
        let scanned =
            native_alnum < crate::images::SPARSE_PAGE_CHARS && self.covered_by_images(scan_like);
        let over_image = |cx: f64, cy: f64| {
            scanned
                && self.images.iter().any(|image| {
                    let b = image.bbox;
                    scan_like(image) && cx >= b[0] && cx <= b[2] && cy >= b[1] && cy <= b[3]
                })
        };
        for (i, glyph) in glyphs.iter().enumerate() {
            if self.glyph_paint[i].1 == Ink::Invisible {
                let (cx, cy) = ((glyph.x0 + glyph.x1) / 2.0, glyph.base + glyph.size * 0.3);
                kept[i] = over_image(cx, cy);
            }
        }
        let rotated_keep: Vec<bool> = self
            .rotated_paint
            .iter()
            .map(|(ink, (x, y))| match ink {
                Ink::Seen => true,
                Ink::Invisible => over_image(*x, *y),
                Ink::ClipOnly => false,
            })
            .collect();
        let mut keep = rotated_keep.iter().copied();
        self.rotated.retain(|_| keep.next().unwrap_or(false));
        let mut keep = rotated_keep.iter().copied();
        self.turns.retain(|_| keep.next().unwrap_or(false));
        let mut keep = rotated_keep.iter().copied();
        self.rotated_paint.retain(|_| keep.next().unwrap_or(false));
        let rotated_layer = self
            .rotated_paint
            .iter()
            .any(|(ink, _)| *ink == Ink::Invisible);
        // Invisible glyphs sitting on visible ones repeat them; keep only the
        // invisible text that stands alone.
        const CELL: f64 = 8.0;
        let cell = |v: f64| (v / CELL).floor() as i64;
        let ink_box = |g: &Glyph| (g.x0, g.base, g.x1, g.base + g.size * 0.8);
        let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = Default::default();
        for (i, glyph) in glyphs.iter().enumerate() {
            if !kept[i] || self.glyph_paint[i].1 != Ink::Seen {
                continue;
            }
            let (x0, y0, x1, y1) = ink_box(glyph);
            if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
                continue;
            }
            for cx in cell(x0)..=cell(x1).min(cell(x0) + 64) {
                for cy in cell(y0)..=cell(y1).min(cell(y0) + 64) {
                    grid.entry((cx, cy)).or_default().push(i);
                }
            }
        }
        let repeats = |glyph: &Glyph| {
            let (x0, y0, x1, y1) = ink_box(glyph);
            let (px, py) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            grid.get(&(cell(px), cell(py))).is_some_and(|ids| {
                ids.iter().any(|&j| {
                    let (a, b, c, d) = ink_box(&glyphs[j]);
                    px >= a && px <= c && py >= b && py <= d
                })
            })
        };
        let drop: Vec<bool> = glyphs
            .iter()
            .enumerate()
            .map(|(i, g)| !kept[i] || (self.glyph_paint[i].1 == Ink::Invisible && repeats(g)))
            .collect();
        let layer = rotated_layer
            || self
                .glyph_paint
                .iter()
                .zip(&drop)
                .any(|((_, ink, _), d)| *ink == Ink::Invisible && !d);
        let glyphs = glyphs
            .into_iter()
            .zip(drop)
            .filter(|(_, d)| !d)
            .map(|(glyph, _)| glyph)
            .collect();
        (glyphs, layer)
    }
}
