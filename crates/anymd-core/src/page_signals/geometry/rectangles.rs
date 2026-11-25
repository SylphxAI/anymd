use lopdf::{Document, Object};

use super::super::pdf_objects::{number, resolve};

pub(super) fn normalize_rect_coords(rect: [f64; 4]) -> [f64; 4] {
    let left = rect[0].min(rect[2]);
    let right = rect[0].max(rect[2]);
    let bottom = rect[1].min(rect[3]);
    let top = rect[1].max(rect[3]);
    [left, bottom, right, top]
}

pub(super) fn vertices_bbox(points: &[(f64, f64)], pad: f64) -> Option<[f64; 4]> {
    let mut left = f64::INFINITY;
    let mut bottom = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut top = f64::NEG_INFINITY;
    for &(x, y) in points {
        left = left.min(x - pad);
        bottom = bottom.min(y - pad);
        right = right.max(x + pad);
        top = top.max(y + pad);
    }
    left.is_finite().then_some([left, bottom, right, top])
}

pub(super) fn box_values(document: &Document, value: &Object) -> Option<[f64; 4]> {
    let values = resolve(document, value).ok()?.as_array().ok()?;
    if values.len() < 4 {
        return None;
    }
    let parsed = [
        number(resolve(document, &values[0]).ok()?)?,
        number(resolve(document, &values[1]).ok()?)?,
        number(resolve(document, &values[2]).ok()?)?,
        number(resolve(document, &values[3]).ok()?)?,
    ];
    parsed
        .iter()
        .all(|value| value.is_finite())
        .then_some(parsed)
}
