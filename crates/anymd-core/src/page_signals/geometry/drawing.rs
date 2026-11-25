use lopdf::{Dictionary, Document};

use super::super::pdf_objects::{number, resolve};
use super::appearance::{annotation_has_normal_appearance, border_style_width};
use super::rectangles::{box_values, normalize_rect_coords, vertices_bbox};

// Without an appearance, pdf.js checks Rect against the line bounds padded by
// twice the border width, then pads the chosen rectangle by the border width.
pub(super) fn line_rect(
    document: &Document,
    dict: &Dictionary,
    rect: Option<[f64; 4]>,
) -> Option<[f64; 4]> {
    if annotation_has_normal_appearance(document, dict) {
        return rect;
    }
    let Some(line) = line_coordinates(document, dict) else {
        return rect;
    };
    let bw = border_style_width(document, dict, rect);
    let line = normalize_rect_coords(line);
    let line_bbox = expand_rect(line, 2.0 * bw);
    Some(expand_rect(intersecting_or_fallback(rect, line_bbox), bw))
}

pub(super) fn polygon_rect(
    document: &Document,
    dict: &Dictionary,
    rect: Option<[f64; 4]>,
) -> Option<[f64; 4]> {
    if annotation_has_normal_appearance(document, dict) {
        return rect;
    }
    points_rect(document, dict, rect, vertices_points(document, dict))
}

pub(super) fn ink_rect(
    document: &Document,
    dict: &Dictionary,
    rect: Option<[f64; 4]>,
) -> Option<[f64; 4]> {
    if annotation_has_normal_appearance(document, dict) {
        return rect;
    }
    points_rect(document, dict, rect, ink_lists_points(document, dict))
}

// Polygon, polyline, and ink use the same fallback as Line but do not apply
// any additional padding to the chosen rectangle.
fn points_rect(
    document: &Document,
    dict: &Dictionary,
    rect: Option<[f64; 4]>,
    points: Option<Vec<(f64, f64)>>,
) -> Option<[f64; 4]> {
    let Some(points) = points else {
        return rect;
    };
    let bw = border_style_width(document, dict, rect);
    let Some(bounds) = vertices_bbox(&points, 2.0 * bw) else {
        return rect;
    };
    Some(intersecting_or_fallback(rect, bounds))
}

fn intersecting_or_fallback(rect: Option<[f64; 4]>, bounds: [f64; 4]) -> [f64; 4] {
    match rect.map(normalize_rect_coords) {
        Some(current) if rects_intersect(current, bounds) => current,
        _ => bounds,
    }
}

fn expand_rect(rect: [f64; 4], pad: f64) -> [f64; 4] {
    let [left, bottom, right, top] = normalize_rect_coords(rect);
    [left - pad, bottom - pad, right + pad, top + pad]
}

fn rects_intersect(a: [f64; 4], b: [f64; 4]) -> bool {
    let a = normalize_rect_coords(a);
    let b = normalize_rect_coords(b);
    a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}

fn line_coordinates(document: &Document, dict: &lopdf::Dictionary) -> Option<[f64; 4]> {
    let value = dict.get(b"L").ok()?;
    box_values(document, value)
}

fn vertices_points(document: &Document, dict: &lopdf::Dictionary) -> Option<Vec<(f64, f64)>> {
    let value = dict.get(b"Vertices").ok()?;
    let values = resolve(document, value).ok()?.as_array().ok()?;
    if values.len() < 2 {
        return None;
    }
    let mut points = Vec::with_capacity(values.len() / 2);
    let mut index = 0;
    while index + 1 < values.len() {
        let x = number(resolve(document, &values[index]).ok()?)?;
        let y = number(resolve(document, &values[index + 1]).ok()?)?;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        points.push((x, y));
        index += 2;
    }
    (!points.is_empty()).then_some(points)
}

fn ink_lists_points(document: &Document, dict: &lopdf::Dictionary) -> Option<Vec<(f64, f64)>> {
    let value = dict.get(b"InkList").ok()?;
    let lists = resolve(document, value).ok()?.as_array().ok()?;
    let mut points = Vec::new();
    for entry in lists {
        let values = resolve(document, entry).ok()?.as_array().ok()?;
        let mut index = 0;
        while index + 1 < values.len() {
            let x = number(resolve(document, &values[index]).ok()?)?;
            let y = number(resolve(document, &values[index + 1]).ok()?)?;
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            points.push((x, y));
            index += 2;
        }
    }
    (!points.is_empty()).then_some(points)
}
