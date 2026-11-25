//! Turns stroked segments and filled boxes into page-space rules.

use pdf_extract::{ColorSpace, OutputError, Path as PdfPath, PathOp, Transform};

use super::collector::{rgb, Collector};
use super::Rule;

/// Rules beyond this many on one page are ignored (charts, hatching).
pub(super) const MAX_RULES_PER_PAGE: usize = 20_000;
/// A filled box at most this thick is a rule, not a box.
const BAR_THICKNESS: f64 = 3.0;

pub(super) fn apply(ctm: &Transform, x: f64, y: f64) -> (f64, f64) {
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
    pub(super) fn push_rule(&mut self, (x0, y0): (f64, f64), (x1, y1): (f64, f64), soft: bool) {
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

    pub(super) fn collect_stroke(
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

    pub(super) fn collect_fill(
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
