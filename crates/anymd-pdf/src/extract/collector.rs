//! Collects glyphs, image placements, and paint order from the content stream.

use crate::MAX_GLYPHS_PER_PAGE;
use pdf_extract::{
    ColorSpace, MediaBox, ObjectId, OutputDev, OutputError, Path as PdfPath, Transform,
};

use super::rules::{apply, MAX_RULES_PER_PAGE};
use super::{normalize_glyph_text, Glyph, Placement, Rule, MAX_PLACEMENTS_PER_PAGE};

/// What a text rendering mode puts on the page.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Ink {
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
    pub(super) glyph_paint: Vec<(Option<[f64; 3]>, Ink, usize)>,
    /// Filled boxes: extent (x0, y0, x1, y1), colour, and when drawn.
    pub(super) boxes: Vec<([f64; 4], [f64; 3], usize)>,
    pub(super) drawn: usize,
    /// For each rotated glyph: its text direction, in quarter turns
    /// counter-clockwise (1, 2 or 3), or 0 for any other angle.
    pub(super) turns: Vec<u8>,
    /// For each rotated glyph: its ink and its page-space position.
    pub(super) rotated_paint: Vec<(Ink, (f64, f64))>,
}

/// A colour as RGB, when its colour space is a device one.
pub(super) fn rgb(colorspace: &ColorSpace, color: &[f64]) -> Option<[f64; 3]> {
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
    pub(super) fn turn_page(&mut self, glyphs: &mut Vec<Glyph>, bottom: &mut f64, top: &mut f64) {
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
        self.collect_stroke(ctm, colorspace, color, path)
    }

    fn fill(
        &mut self,
        ctm: &Transform,
        colorspace: &ColorSpace,
        color: &[f64],
        path: &PdfPath,
    ) -> Result<(), OutputError> {
        self.collect_fill(ctm, colorspace, color, path)
    }
}
