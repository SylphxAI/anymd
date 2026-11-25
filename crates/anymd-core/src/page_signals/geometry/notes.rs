use lopdf::{Dictionary, Document};

use super::appearance::text_annotation_has_appearance;

/// pdf.js TextAnnotation DEFAULT_ICON_SIZE.
const TEXT_ANNOTATION_ICON_SIZE: f64 = 22.0;

pub(super) fn note_rect(
    document: &Document,
    dict: &Dictionary,
    rect: Option<[f64; 4]>,
) -> Option<[f64; 4]> {
    if text_annotation_has_appearance(document, dict) {
        return rect;
    }
    // Without an appearance, pdf.js anchors its 22x22 icon at the top left.
    let [x1, y1, x2, y2] = rect?;
    let left = x1.min(x2);
    let top = y1.max(y2);
    Some([
        left,
        top - TEXT_ANNOTATION_ICON_SIZE,
        left + TEXT_ANNOTATION_ICON_SIZE,
        top,
    ])
}

pub(super) fn popup_rect(rect: Option<[f64; 4]>) -> Option<[f64; 4]> {
    // pdf.js omits the bounding box for a popup with zero width or height.
    rect.filter(|&[left, bottom, right, top]| {
        let width = left.max(right) - left.min(right);
        let height = bottom.max(top) - bottom.min(top);
        width != 0.0 && height != 0.0
    })
}
