//! Glyphs and ruling lines from the PDF content stream (via `pdf-extract`).

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::{MAX_GLYPHS_PER_PAGE, MAX_WORKERS};
use pdf_extract::{
    output_doc_page, ColorSpace, Document, MediaBox, ObjectId, OutputDev, OutputError,
    Path as PdfPath, PathOp, Transform,
};

#[derive(Debug, Clone)]
pub(crate) struct Glyph {
    /// Start and end along the text direction.
    pub(crate) x0: f64,
    pub(crate) x1: f64,
    /// Baseline position across the text direction (larger = higher on page).
    pub(crate) base: f64,
    pub(crate) size: f64,
    pub(crate) text: String,
    pub(crate) space: bool,
}

/// A straight horizontal or vertical line drawn on the page: a stroked
/// segment, a thin filled bar, or an edge of a filled box. `at` is the y of a
/// horizontal rule or the x of a vertical one; `from..to` is its extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rule {
    pub(crate) horizontal: bool,
    pub(crate) at: f64,
    pub(crate) from: f64,
    pub(crate) to: f64,
    /// An edge of a shaded box rather than a drawn line. A shaded column or
    /// band does not divide cells the way a drawn line does.
    pub(crate) soft: bool,
}

/// An image XObject painted on a page: its object and page-space extent
/// (x0, y0, x1, y1; y up).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placement {
    pub(crate) object: ObjectId,
    pub(crate) bbox: [f64; 4],
}

/// Images painted on one page beyond this many are ignored.
const MAX_PLACEMENTS_PER_PAGE: usize = 256;

pub(crate) struct RawPage {
    pub(crate) number: u32,
    pub(crate) bottom: f64,
    pub(crate) top: f64,
    pub(crate) glyphs: Result<Vec<Glyph>, String>,
    pub(crate) rotated: Vec<Glyph>,
    pub(crate) rules: Vec<Rule>,
    /// Glyphs made from OCR word boxes: even widths inside a word say nothing
    /// about the font being monospace.
    pub(crate) ocr: bool,
    /// Image XObjects painted on the page.
    pub(crate) images: Vec<Placement>,
    /// The page's area in square points (0 when unknown).
    pub(crate) area: f64,
    /// Embedded images kept for the output, placed in reading order.
    pub(crate) figures: Vec<crate::images::Figure>,
    /// The page's text includes an invisible OCR text layer over a scan.
    pub(crate) invisible_layer: bool,
}

/// What a text rendering mode puts on the page.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Ink {
    #[default]
    Seen,
    /// Mode 3: draws nothing. Scanners and OCR tools store their text layer
    /// this way, over the page image; elsewhere it is hidden text.
    Invisible,
    /// Mode 7: adds to the clip and draws nothing; never read.
    ClipOnly,
}

#[derive(Default)]
pub(crate) struct Collector {
    pub(crate) media: Option<MediaBox>,
    pub(crate) images: Vec<Placement>,
    pub(crate) glyphs: Vec<Glyph>,
    pub(crate) rotated: Vec<Glyph>,
    pub(crate) rules: Vec<Rule>,
    /// Paint of the text being shown: its colour (when known) and whether
    /// its rendering mode draws nothing.
    paint: (Option<[f64; 3]>, Ink),
    /// For each upright glyph: its paint colour, its ink, and when it was
    /// drawn.
    glyph_paint: Vec<(Option<[f64; 3]>, Ink, usize)>,
    /// Filled boxes: extent (x0, y0, x1, y1), colour, and when drawn.
    boxes: Vec<([f64; 4], [f64; 3], usize)>,
    drawn: usize,
    /// For each rotated glyph: its text direction, in quarter turns
    /// counter-clockwise (1, 2 or 3), or 0 for any other angle.
    turns: Vec<u8>,
    /// For each rotated glyph: its ink and its page-space position.
    rotated_paint: Vec<(Ink, (f64, f64))>,
}

/// A colour as RGB, when its colour space is a device one.
fn rgb(colorspace: &ColorSpace, color: &[f64]) -> Option<[f64; 3]> {
    match (colorspace, color) {
        (ColorSpace::DeviceGray | ColorSpace::CalGray(_), [g, ..]) => Some([*g, *g, *g]),
        (ColorSpace::DeviceRGB | ColorSpace::CalRGB(_), [r, g, b, ..]) => Some([*r, *g, *b]),
        (ColorSpace::ICCBased(_), [r, g, b]) => Some([*r, *g, *b]),
        (ColorSpace::ICCBased(_), [g]) => Some([*g, *g, *g]),
        (ColorSpace::DeviceCMYK, [c, m, y, k, ..]) => Some([
            (1.0 - c) * (1.0 - k),
            (1.0 - m) * (1.0 - k),
            (1.0 - y) * (1.0 - k),
        ]),
        _ => None,
    }
}

impl Collector {
    /// A page whose text mostly runs in one turned direction (a landscape
    /// table on a portrait page): lay that text out as the page, in its own
    /// frame, with the rules turned to match. Upright glyphs become the
    /// page's side text instead.
    fn turn_page(&mut self, glyphs: &mut Vec<Glyph>, bottom: &mut f64, top: &mut f64) {
        let mut counts = [0usize; 4];
        for turn in &self.turns {
            counts[*turn as usize] += 1;
        }
        let Some((turn, &count)) = counts.iter().enumerate().skip(1).max_by_key(|(_, c)| **c)
        else {
            return;
        };
        if count < 50 || count * 10 < (glyphs.len() + self.rotated.len()) * 6 {
            return;
        }
        let (dx, dy) = match turn {
            1 => (0.0, 1.0),
            2 => (-1.0, 0.0),
            _ => (0.0, -1.0),
        };
        let frame = |x: f64, y: f64| (x * dx + y * dy, -x * dy + y * dx);
        let mut turned = Vec::with_capacity(count);
        let mut rest = std::mem::take(glyphs);
        for (glyph, t) in std::mem::take(&mut self.rotated)
            .into_iter()
            .zip(&self.turns)
        {
            if *t as usize == turn {
                turned.push(glyph);
            } else {
                rest.push(glyph);
            }
        }
        // Image extents are not turned with the text; leave them out.
        self.images.clear();
        let rules = std::mem::take(&mut self.rules);
        for rule in rules {
            let (a, b) = if rule.horizontal {
                ((rule.from, rule.at), (rule.to, rule.at))
            } else {
                ((rule.at, rule.from), (rule.at, rule.to))
            };
            self.push_rule(frame(a.0, a.1), frame(b.0, b.1), rule.soft);
        }
        if let Some(media) = self.media {
            let corners = [
                frame(media.llx, media.lly),
                frame(media.urx, media.lly),
                frame(media.llx, media.ury),
                frame(media.urx, media.ury),
            ];
            *bottom = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
            *top = corners
                .iter()
                .map(|c| c.1)
                .fold(f64::NEG_INFINITY, f64::max);
        }
        *glyphs = turned;
        self.rotated = rest;
        self.turns.clear();
    }

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

/// Rules beyond this many on one page are ignored (charts, hatching).
const MAX_RULES_PER_PAGE: usize = 20_000;
/// A filled box at most this thick is a rule, not a box.
const BAR_THICKNESS: f64 = 3.0;

fn apply(ctm: &Transform, x: f64, y: f64) -> (f64, f64) {
    (
        ctm.m11 * x + ctm.m21 * y + ctm.m31,
        ctm.m12 * x + ctm.m22 * y + ctm.m32,
    )
}

/// Whether a colour paints (almost) white, which draws nothing visible on a
/// white page.
fn is_white(colorspace: &ColorSpace, color: &[f64]) -> bool {
    match colorspace {
        ColorSpace::DeviceGray | ColorSpace::CalGray(_) => color.first().is_some_and(|v| *v > 0.95),
        ColorSpace::DeviceRGB | ColorSpace::CalRGB(_) => {
            color.len() >= 3 && color[..3].iter().all(|v| *v > 0.95)
        }
        ColorSpace::DeviceCMYK => color.len() >= 4 && color[..4].iter().all(|v| *v < 0.05),
        _ => false,
    }
}

impl Collector {
    fn push_rule(&mut self, (x0, y0): (f64, f64), (x1, y1): (f64, f64), soft: bool) {
        if self.rules.len() >= MAX_RULES_PER_PAGE || ![x0, y0, x1, y1].iter().all(|v| v.is_finite())
        {
            return;
        }
        let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
        if dy <= 0.5 && dx >= 1.0 {
            self.rules.push(Rule {
                horizontal: true,
                at: (y0 + y1) / 2.0,
                from: x0.min(x1),
                to: x0.max(x1),
                soft,
            });
        } else if dx <= 0.5 && dy >= 1.0 {
            self.rules.push(Rule {
                horizontal: false,
                at: (x0 + x1) / 2.0,
                from: y0.min(y1),
                to: y0.max(y1),
                soft,
            });
        }
    }

    /// The subpaths of a path in page space: each subpath's straight edges,
    /// and its bounding box when it is an axis-aligned rectangle.
    fn subpaths(ctm: &Transform, path: &PdfPath) -> Vec<Subpath> {
        let mut out: Vec<Subpath> = Vec::new();
        let mut open: Option<Subpath> = None;
        let mut start = None;
        let mut current = None;
        for op in &path.ops {
            match *op {
                PathOp::MoveTo(x, y) => {
                    out.extend(open.take());
                    let point = apply(ctm, x, y);
                    open = Some(Subpath::default());
                    start = Some(point);
                    current = Some(point);
                }
                PathOp::LineTo(x, y) => {
                    let point = apply(ctm, x, y);
                    let subpath = open.get_or_insert_with(Subpath::default);
                    if let Some(from) = current {
                        subpath.edges.push((from, point));
                    }
                    current = Some(point);
                }
                PathOp::CurveTo(_, _, _, _, x, y) => {
                    // Curves are never rules; they break the current line.
                    open.get_or_insert_with(Subpath::default).curved = true;
                    current = Some(apply(ctm, x, y));
                }
                PathOp::Rect(x, y, w, h) => {
                    out.extend(open.take());
                    let corners = [
                        apply(ctm, x, y),
                        apply(ctm, x + w, y),
                        apply(ctm, x + w, y + h),
                        apply(ctm, x, y + h),
                    ];
                    out.push(Subpath {
                        edges: (0..4).map(|i| (corners[i], corners[(i + 1) % 4])).collect(),
                        curved: false,
                        closed: true,
                    });
                    start = None;
                    current = None;
                }
                PathOp::Close => {
                    if let Some(subpath) = open.as_mut() {
                        if let (Some(from), Some(to)) = (current, start) {
                            subpath.edges.push((from, to));
                        }
                        subpath.closed = true;
                    }
                    current = start;
                }
            }
        }
        out.extend(open);
        out
    }
}

#[derive(Default)]
struct Subpath {
    edges: Vec<((f64, f64), (f64, f64))>,
    curved: bool,
    closed: bool,
}

impl Subpath {
    /// The bounding box of a closed, straight, axis-aligned outline (a box
    /// drawn with `re` or with four lines).
    fn as_box(&self) -> Option<[f64; 4]> {
        if self.curved || self.edges.len() < 3 || self.edges.len() > 5 {
            return None;
        }
        let straight = self
            .edges
            .iter()
            .all(|(a, b)| (a.0 - b.0).abs() < 0.5 || (a.1 - b.1).abs() < 0.5);
        let first = self.edges.first()?.0;
        let last = self.edges.last()?.1;
        let closes =
            self.closed || ((first.0 - last.0).abs() < 0.5 && (first.1 - last.1).abs() < 0.5);
        if !(straight && closes) {
            return None;
        }
        let xs = self.edges.iter().flat_map(|(a, b)| [a.0, b.0]);
        let ys = self.edges.iter().flat_map(|(a, b)| [a.1, b.1]);
        Some([
            xs.clone().fold(f64::INFINITY, f64::min),
            ys.clone().fold(f64::INFINITY, f64::min),
            xs.fold(f64::NEG_INFINITY, f64::max),
            ys.fold(f64::NEG_INFINITY, f64::max),
        ])
    }
}

impl OutputDev for Collector {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.media = Some(*media_box);
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        character: &str,
    ) -> Result<(), OutputError> {
        if self.glyphs.len() + self.rotated.len() >= MAX_GLYPHS_PER_PAGE {
            return Err(OutputError::IoError(std::io::Error::other(
                "page has too many glyphs",
            )));
        }
        let values = [
            trm.m11, trm.m12, trm.m21, trm.m22, trm.m31, trm.m32, width, font_size,
        ];
        if !values.iter().all(|value| value.is_finite()) {
            return Ok(());
        }
        let text = normalize_glyph_text(character);
        if text.is_empty() {
            return Ok(());
        }
        let size = (trm.m21.hypot(trm.m22) * font_size).abs();
        if size <= 0.1 || size > 2000.0 {
            return Ok(());
        }
        let scale = trm.m11.hypot(trm.m12);
        if scale <= 0.0 {
            return Ok(());
        }
        let (dx, dy) = (trm.m11 / scale, trm.m12 / scale);
        let space = text.chars().all(char::is_whitespace);
        // Character spacing (Tc) is part of the advance; word spacing is not.
        let tracking = if space { 0.0 } else { spacing };
        let mut advance = ((width * font_size + tracking) * scale).abs();
        if advance < size * 0.05 {
            // Fonts without widths would otherwise glue every glyph together.
            advance = size * 0.5;
        }
        let upright = dx > 0.9 && dy.abs() < 0.2;
        let (along, across) = if upright {
            (trm.m31, trm.m32)
        } else {
            // Project onto the rotated text direction.
            (trm.m31 * dx + trm.m32 * dy, -trm.m31 * dy + trm.m32 * dx)
        };
        let glyph = Glyph {
            x0: along,
            x1: along + advance,
            base: across,
            size,
            text: if space { " ".into() } else { text },
            space,
        };
        if upright {
            self.glyphs.push(glyph);
            self.drawn += 1;
            self.glyph_paint
                .push((self.paint.0, self.paint.1, self.drawn));
        } else {
            let turn = if dy > 0.9 && dx.abs() < 0.2 {
                1
            } else if dx < -0.9 && dy.abs() < 0.2 {
                2
            } else if dy < -0.9 && dx.abs() < 0.2 {
                3
            } else {
                0
            };
            self.rotated.push(glyph);
            self.turns.push(turn);
            self.rotated_paint
                .push((self.paint.1, (trm.m31, trm.m32 + size * 0.3)));
        }
        Ok(())
    }

    fn image(&mut self, ctm: &Transform, object: Option<ObjectId>) -> Result<(), OutputError> {
        // An image can sit behind text of any colour: it counts as a box of
        // no colour, which hides nothing.
        let corners = [
            apply(ctm, 0.0, 0.0),
            apply(ctm, 1.0, 0.0),
            apply(ctm, 0.0, 1.0),
            apply(ctm, 1.0, 1.0),
        ];
        let xs = corners.map(|c| c.0);
        let ys = corners.map(|c| c.1);
        let extent = [
            xs.iter().cloned().fold(f64::INFINITY, f64::min),
            ys.iter().cloned().fold(f64::INFINITY, f64::min),
            xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        ];
        self.drawn += 1;
        if self.boxes.len() < MAX_RULES_PER_PAGE {
            self.boxes.push((extent, [f64::NAN; 3], self.drawn));
        }
        if let Some(object) = object {
            if self.images.len() < MAX_PLACEMENTS_PER_PAGE && extent.iter().all(|v| v.is_finite()) {
                self.images.push(Placement {
                    object,
                    bbox: extent,
                });
            }
        }
        Ok(())
    }

    fn text_paint(
        &mut self,
        colorspace: &ColorSpace,
        color: &[f64],
        render_mode: i64,
    ) -> Result<(), OutputError> {
        let ink = match render_mode {
            3 => Ink::Invisible,
            7 => Ink::ClipOnly,
            _ => Ink::Seen,
        };
        self.paint = (rgb(colorspace, color), ink);
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn stroke(
        &mut self,
        ctm: &Transform,
        colorspace: &ColorSpace,
        color: &[f64],
        path: &PdfPath,
    ) -> Result<(), OutputError> {
        if is_white(colorspace, color) {
            return Ok(());
        }
        for subpath in Self::subpaths(ctm, path) {
            if !subpath.curved {
                for (from, to) in subpath.edges {
                    self.push_rule(from, to, false);
                }
            }
        }
        Ok(())
    }

    fn fill(
        &mut self,
        ctm: &Transform,
        colorspace: &ColorSpace,
        color: &[f64],
        path: &PdfPath,
    ) -> Result<(), OutputError> {
        let white = is_white(colorspace, color);
        for subpath in Self::subpaths(ctm, path) {
            if white {
                // Draws no line, but hides text of its own colour.
                if let (Some(extent), Some(fill)) = (subpath.as_box(), rgb(colorspace, color)) {
                    self.drawn += 1;
                    if self.boxes.len() < MAX_RULES_PER_PAGE {
                        self.boxes.push((extent, fill, self.drawn));
                    }
                }
                continue;
            }
            match subpath.as_box() {
                // A thin bar is one rule along its middle.
                Some([x0, y0, x1, y1]) if y1 - y0 <= BAR_THICKNESS && x1 - x0 > y1 - y0 => {
                    self.push_rule((x0, (y0 + y1) / 2.0), (x1, (y0 + y1) / 2.0), false);
                }
                Some([x0, y0, x1, y1]) if x1 - x0 <= BAR_THICKNESS => {
                    self.push_rule(((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1), false);
                }
                // A shaded box (a header band, a cell background): its edges
                // separate what is inside from what is outside.
                Some(extent) => {
                    if let Some(fill) = rgb(colorspace, color) {
                        self.drawn += 1;
                        if self.boxes.len() < MAX_RULES_PER_PAGE {
                            self.boxes.push((extent, fill, self.drawn));
                        }
                    }
                    for (from, to) in subpath.edges {
                        self.push_rule(from, to, true);
                    }
                }
                None => {}
            }
        }
        Ok(())
    }
}

pub(crate) fn normalize_glyph_text(character: &str) -> String {
    let mut out = String::with_capacity(character.len());
    for ch in character.chars() {
        match ch {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{3000}' | '\t' => out.push(' '),
            '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{FEFF}' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn extract_pages(doc: &Document, selected: &[u32]) -> Vec<RawPage> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, MAX_WORKERS)
        .min(selected.len().max(1));
    let extract_one = |number: u32| -> RawPage {
        let mut collector = Collector::default();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            output_doc_page(doc, &mut collector, number)
        }));
        let scan_like = |placement: &Placement| crate::images::is_scan_like(doc, placement);
        let mut invisible_layer = false;
        let glyphs = match outcome {
            Ok(Ok(())) => {
                let (glyphs, layer) = collector.visible_glyphs(&scan_like);
                invisible_layer = layer;
                Ok(glyphs)
            }
            Ok(Err(err)) => Err(format!("page {number}: text extraction failed ({err})")),
            Err(_) => Err(format!(
                "page {number}: text extraction failed (malformed font or content)"
            )),
        };
        let area = collector
            .media
            .map(|media| ((media.urx - media.llx) * (media.ury - media.lly)).abs())
            .filter(|area| area.is_finite())
            .unwrap_or(0.0);
        let (mut bottom, mut top) = collector
            .media
            .map(|media| (media.lly.min(media.ury), media.lly.max(media.ury)))
            .unwrap_or((0.0, 792.0));
        let glyphs = glyphs.map(|mut glyphs| {
            collector.turn_page(&mut glyphs, &mut bottom, &mut top);
            glyphs
        });
        RawPage {
            number,
            bottom,
            top,
            glyphs,
            rotated: collector.rotated,
            rules: collector.rules,
            ocr: false,
            images: collector.images,
            area,
            figures: Vec::new(),
            invisible_layer,
        }
    };
    if workers <= 1 {
        return selected.iter().map(|&number| extract_one(number)).collect();
    }
    let mut results: Vec<Option<RawPage>> = (0..selected.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let extract_one = &extract_one;
                scope.spawn(move || {
                    selected
                        .iter()
                        .enumerate()
                        .skip(worker)
                        .step_by(workers)
                        .map(|(index, &number)| (index, extract_one(number)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            if let Ok(pages) = handle.join() {
                for (index, page) in pages {
                    results[index] = Some(page);
                }
            }
        }
    });
    results
        .into_iter()
        .zip(selected)
        .map(|(page, &number)| {
            page.unwrap_or(RawPage {
                number,
                bottom: 0.0,
                top: 792.0,
                glyphs: Err(format!("page {number}: text extraction failed")),
                rotated: Vec::new(),
                rules: Vec::new(),
                ocr: false,
                images: Vec::new(),
                area: 0.0,
                figures: Vec::new(),
                invisible_layer: false,
            })
        })
        .collect()
}
