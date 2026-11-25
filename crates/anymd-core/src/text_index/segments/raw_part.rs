//! Raw glyph extents and the text parts emitted by PDF extraction.

use pdf_extract::Transform;

use super::super::PositionedTextItem;

/// A glyph's extent along the text direction and its font size, kept beside
/// each character so word spaces can be inferred from real glyph gaps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::text_index) struct GlyphExtent {
    pub(in crate::text_index) x0: f64,
    pub(in crate::text_index) x1: f64,
    pub(in crate::text_index) size: f64,
}

impl GlyphExtent {
    /// Same projection as the anymd-pdf layout engine: start along the text
    /// direction, advance including character spacing (Tc) but not word
    /// spacing, and a half-em fallback for fonts without widths.
    pub(super) fn from_character(
        trm: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        character: &str,
    ) -> Option<Self> {
        let values = [
            trm.m11, trm.m12, trm.m21, trm.m22, trm.m31, trm.m32, width, spacing, font_size,
        ];
        if !values.into_iter().all(f64::is_finite) {
            return None;
        }
        let size = (trm.m21.hypot(trm.m22) * font_size).abs();
        let scale = trm.m11.hypot(trm.m12);
        if size <= 0.1 || scale <= 0.0 {
            return None;
        }
        let (dx, dy) = (trm.m11 / scale, trm.m12 / scale);
        let tracking = if character.chars().all(char::is_whitespace) {
            0.0
        } else {
            spacing
        };
        let mut advance = ((width * font_size + tracking) * scale).abs();
        if advance < size * 0.05 {
            advance = size * 0.5;
        }
        let along = if dx > 0.9 && dy.abs() < 0.2 {
            trm.m31
        } else {
            trm.m31 * dx + trm.m32 * dy
        };
        let extent = Self {
            x0: along,
            x1: along + advance,
            size,
        };
        [extent.x0, extent.x1]
            .into_iter()
            .all(f64::is_finite)
            .then_some(extent)
    }
}

#[derive(Debug)]
pub(in crate::text_index) struct RawTextPart {
    pub(in crate::text_index) item: PositionedTextItem,
    /// One entry per `item.chars` entry.
    pub(in crate::text_index) glyphs: Vec<Option<GlyphExtent>>,
    pub(in crate::text_index) x: Option<f64>,
    pub(in crate::text_index) y: Option<f64>,
    pub(in crate::text_index) right: Option<f64>,
}

impl RawTextPart {
    pub(super) fn empty() -> Self {
        Self {
            item: PositionedTextItem {
                text: String::new(),
                bounding_box: None,
                chars: Vec::new(),
                runs: Vec::new(),
            },
            glyphs: Vec::new(),
            x: None,
            y: None,
            right: None,
        }
    }
}
