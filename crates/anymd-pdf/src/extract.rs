//! Glyphs and ruling lines from the PDF content stream (via `pdf-extract`).

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::{MAX_GLYPHS_PER_PAGE, MAX_WORKERS};
use pdf_extract::{output_doc_page, ColorSpace, Document, MediaBox, OutputDev, OutputError, Path as PdfPath, PathOp, Transform};

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
}

#[derive(Default)]
pub(crate) struct Collector {
    pub(crate) media: Option<MediaBox>,
    pub(crate) glyphs: Vec<Glyph>,
    pub(crate) rotated: Vec<Glyph>,
    pub(crate) rules: Vec<Rule>,
    /// Paint of the text being shown: its colour (when known) and whether
    /// its rendering mode draws nothing.
    paint: (Option<[f64; 3]>, bool),
    /// For each upright glyph: its paint colour, whether it is invisible,
    /// and when it was drawn.
    glyph_paint: Vec<(Option<[f64; 3]>, bool, usize)>,
    /// Filled boxes: extent (x0, y0, x1, y1), colour, and when drawn.
    boxes: Vec<([f64; 4], [f64; 3], usize)>,
    drawn: usize,
}

/// A colour as RGB, when its colour space is a device one.
fn rgb(colorspace: &ColorSpace, color: &[f64]) -> Option<[f64; 3]> {
    match (colorspace, color) {
        (ColorSpace::DeviceGray | ColorSpace::CalGray(_), [g, ..]) => Some([*g, *g, *g]),
        (ColorSpace::DeviceRGB | ColorSpace::CalRGB(_), [r, g, b, ..]) => Some([*r, *g, *b]),
        (ColorSpace::ICCBased(_), [r, g, b]) => Some([*r, *g, *b]),
        (ColorSpace::ICCBased(_), [g]) => Some([*g, *g, *g]),
        (ColorSpace::DeviceCMYK, [c, m, y, k, ..]) => {
            Some([(1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k)])
        }
        _ => None,
    }
}

impl Collector {
    /// Glyphs a reader cannot see: drawn in an invisible rendering mode, or in
    /// the same colour as the box they sit on (text hidden in a table cell or
    /// on a coloured panel). They are dropped, as a reader never sees them.
    pub(crate) fn visible_glyphs(&mut self) -> Vec<Glyph> {
        let glyphs = std::mem::take(&mut self.glyphs);
        if self.glyph_paint.len() != glyphs.len() {
            return glyphs;
        }
        glyphs
            .into_iter()
            .zip(&self.glyph_paint)
            .filter(|(glyph, (color, invisible, when))| {
                if *invisible {
                    return false;
                }
                let Some(color) = color else { return true };
                let (cx, cy) = ((glyph.x0 + glyph.x1) / 2.0, glyph.base + glyph.size * 0.3);
                let under = self
                    .boxes
                    .iter()
                    .rev()
                    .find(|(b, _, at)| at < when && cx >= b[0] && cx <= b[2] && cy >= b[1] && cy <= b[3]);
                !under.is_some_and(|(_, fill, _)| {
                    fill.iter().zip(color).all(|(a, b)| (a - b).abs() < 0.06)
                })
            })
            .map(|(glyph, _)| glyph)
            .collect()
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
        if self.rules.len() >= MAX_RULES_PER_PAGE
            || ![x0, y0, x1, y1].iter().all(|v| v.is_finite())
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
        let closes = self.closed || ((first.0 - last.0).abs() < 0.5 && (first.1 - last.1).abs() < 0.5);
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
            self.glyph_paint.push((self.paint.0, self.paint.1, self.drawn));
        } else {
            self.rotated.push(glyph);
        }
        Ok(())
    }

    fn image(&mut self, ctm: &Transform) -> Result<(), OutputError> {
        // An image can sit behind text of any colour: it counts as a box of
        // no colour, which hides nothing.
        let corners = [apply(ctm, 0.0, 0.0), apply(ctm, 1.0, 0.0), apply(ctm, 0.0, 1.0), apply(ctm, 1.0, 1.0)];
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
        Ok(())
    }

    fn text_paint(
        &mut self,
        colorspace: &ColorSpace,
        color: &[f64],
        render_mode: i64,
    ) -> Result<(), OutputError> {
        self.paint = (rgb(colorspace, color), render_mode == 3 || render_mode == 7);
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
        let glyphs = match outcome {
            Ok(Ok(())) => Ok(collector.visible_glyphs()),
            Ok(Err(err)) => Err(format!("page {number}: text extraction failed ({err})")),
            Err(_) => Err(format!(
                "page {number}: text extraction failed (malformed font or content)"
            )),
        };
        let (bottom, top) = collector
            .media
            .map(|media| (media.lly.min(media.ury), media.lly.max(media.ury)))
            .unwrap_or((0.0, 792.0));
        RawPage {
            number,
            bottom,
            top,
            glyphs,
            rotated: collector.rotated,
            rules: collector.rules,
            ocr: false,
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
            })
        })
        .collect()
}
