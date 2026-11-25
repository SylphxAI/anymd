use lopdf::{Dictionary, Document};

use super::super::pdf_objects::{number, resolve};
use super::appearance::text_markup_should_use_quad_bbox;
use super::rectangles::vertices_bbox;

// When pdf.js synthesizes a markup appearance, it uses the quad union or,
// for Squiggly, the union of strips along the bottom of the quads.
pub(super) fn markup_rect(
    document: &Document,
    dict: &Dictionary,
    subtype: &str,
    rect: Option<[f64; 4]>,
) -> Option<[f64; 4]> {
    if !text_markup_should_use_quad_bbox(document, dict, subtype) {
        return rect;
    }
    let Some(points) = quad_points(document, dict) else {
        return rect;
    };
    let bounds = if subtype == "Squiggly" {
        squiggly_quad_bbox(&points)
    } else {
        vertices_bbox(&points, 0.0)
    };
    bounds.or(rect)
}

fn quad_points(document: &Document, dict: &lopdf::Dictionary) -> Option<Vec<(f64, f64)>> {
    // pdf.js getQuadPoints: array length > 0 and multiple of 8; each group becomes
    // axis-aligned corners [minX,maxY,maxX,maxY,minX,minY,maxX,minY].
    let value = dict.get(b"QuadPoints").ok()?;
    let values = resolve(document, value).ok()?.as_array().ok()?;
    if values.is_empty() || values.len() % 8 != 0 {
        return None;
    }
    let mut points = Vec::with_capacity(values.len() / 2);
    let mut index = 0;
    while index + 7 < values.len() {
        let mut xs = [0.0; 4];
        let mut ys = [0.0; 4];
        for corner in 0..4 {
            let x = number(resolve(document, &values[index + corner * 2]).ok()?)?;
            let y = number(resolve(document, &values[index + corner * 2 + 1]).ok()?)?;
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            xs[corner] = x;
            ys[corner] = y;
        }
        let min_x = xs[0].min(xs[1]).min(xs[2]).min(xs[3]);
        let max_x = xs[0].max(xs[1]).max(xs[2]).max(xs[3]);
        let min_y = ys[0].min(ys[1]).min(ys[2]).min(ys[3]);
        let max_y = ys[0].max(ys[1]).max(ys[2]).max(ys[3]);
        // normalized pdf.js order: TL, TR, BL, BR
        points.push((min_x, max_y));
        points.push((max_x, max_y));
        points.push((min_x, min_y));
        points.push((max_x, min_y));
        index += 8;
    }
    (!points.is_empty()).then_some(points)
}

fn squiggly_quad_bbox(points: &[(f64, f64)]) -> Option<[f64; 4]> {
    // pdf.js SquigglyAnnotation pointsCallback over normalized quads
    // [minX,maxY, maxX,maxY, minX,minY, maxX,minY] returns
    // [minX, minY-2*dy, maxX, minY+2*dy] with dy=(maxY-minY)/6, unioned.
    if points.len() < 4 || !points.len().is_multiple_of(4) {
        return None;
    }
    let mut left = f64::INFINITY;
    let mut bottom = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut top = f64::NEG_INFINITY;
    let mut index = 0;
    while index + 3 < points.len() {
        let (min_x, max_y) = points[index];
        let (max_x, _) = points[index + 1];
        let (_, min_y) = points[index + 2];
        let dy = (max_y - min_y) / 6.0;
        left = left.min(min_x);
        bottom = bottom.min(min_y - 2.0 * dy);
        right = right.max(max_x);
        top = top.max(min_y + 2.0 * dy);
        index += 4;
    }
    left.is_finite().then_some([left, bottom, right, top])
}
