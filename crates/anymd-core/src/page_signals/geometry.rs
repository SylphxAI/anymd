use lopdf::{Document, Object};

use super::pdf_objects::{number, resolve};

mod appearance;

use appearance::{
    annotation_has_normal_appearance, border_style_width, text_annotation_has_appearance,
    text_markup_should_use_quad_bbox,
};

pub(super) fn annotation_rect(
    document: &Document,
    dict: &lopdf::Dictionary,
    subtype: Option<&str>,
) -> Option<[f64; 4]> {
    let mut rect = dict
        .get(b"Rect")
        .ok()
        .and_then(|value| box_values(document, value));
    // pdf.js normalizes Rect first (lookupNormalRect), then TextAnnotation without
    // appearance forces a 22x22 icon box anchored at the top-left of that box:
    // bottom = top - 22, right = left + 22.
    if subtype == Some("Text") && !text_annotation_has_appearance(document, dict) {
        if let Some([x1, y1, x2, y2]) = rect {
            let left = x1.min(x2);
            let top = y1.max(y2);
            rect = Some([
                left,
                top - TEXT_ANNOTATION_ICON_SIZE,
                left + TEXT_ANNOTATION_ICON_SIZE,
                top,
            ]);
        }
    }
    // pdf.js PopupAnnotation: after lookupNormalRect, width==0 || height==0
    // sets data.rect = null, so public TS omits bounding_box.
    if subtype == Some("Popup") {
        if let Some([left, bottom, right, top]) = rect {
            let width = left.max(right) - left.min(right);
            let height = bottom.max(top) - bottom.min(top);
            if width == 0.0 || height == 0.0 {
                rect = None;
            }
        }
    }

    // pdf.js LineAnnotation without appearance:
    // - compute L-normalized bbox expanded by 2*borderWidth (default width 1)
    // - if Rect does not intersect that bbox, replace Rect with the L bbox
    // - public rect is then expanded by borderWidth (default appearance path)
    if subtype == Some("Line") && !annotation_has_normal_appearance(document, dict) {
        if let Some(line) = line_coordinates(document, dict) {
            let bw = border_style_width(document, dict, rect);
            let line = normalize_rect_coords(line);
            let line_bbox = expand_rect(line, 2.0 * bw);
            let current = rect.map(normalize_rect_coords);
            let base = match current {
                Some(r) if rects_intersect(r, line_bbox) => r,
                _ => line_bbox,
            };
            rect = Some(expand_rect(base, bw));
        }
    }

    // pdf.js PolylineAnnotation/PolygonAnnotation without appearance:
    // - vertices bbox expanded by 2*borderWidth (default width 1)
    // - if Rect does not intersect that bbox, replace Rect with the vertices bbox
    // - public rect is the resulting rectangle (no additional borderWidth expand)
    if matches!(subtype, Some("PolyLine") | Some("Polygon"))
        && !annotation_has_normal_appearance(document, dict)
    {
        if let Some(points) = vertices_points(document, dict) {
            let bw = border_style_width(document, dict, rect);
            if let Some(vertices_box) = vertices_bbox(&points, 2.0 * bw) {
                let current = rect.map(normalize_rect_coords);
                rect = Some(match current {
                    Some(r) if rects_intersect(r, vertices_box) => r,
                    _ => vertices_box,
                });
            }
        }
    }

    // pdf.js InkAnnotation without appearance:
    // - ink-list points bbox expanded by 2*borderWidth (default width 1)
    // - if Rect does not intersect that bbox, replace Rect with the ink bbox
    // - public rect is the resulting rectangle (no additional borderWidth expand)
    if subtype == Some("Ink") && !annotation_has_normal_appearance(document, dict) {
        if let Some(points) = ink_lists_points(document, dict) {
            let bw = border_style_width(document, dict, rect);
            if let Some(ink_box) = vertices_bbox(&points, 2.0 * bw) {
                let current = rect.map(normalize_rect_coords);
                rect = Some(match current {
                    Some(r) if rects_intersect(r, ink_box) => r,
                    _ => ink_box,
                });
            }
        }
    }

    // pdf.js Highlight/Underline/Squiggly/StrikeOut:
    // - when synthesizing default appearance from QuadPoints, public data.rect
    //   is rewritten by _setDefaultAppearance from pointsCallback returns.
    // - Highlight/Underline/StrikeOut return the axis-aligned quad union.
    // - Squiggly returns a bottom-strip around the squiggle:
    //   [minX, minY-2*dy, maxX, minY+2*dy] where dy=(maxY-minY)/6.
    // - Highlight additionally ignores appearance streams without ExtGState.
    if matches!(
        subtype,
        Some("Highlight") | Some("Underline") | Some("Squiggly") | Some("StrikeOut")
    ) {
        let subtype_name = subtype.unwrap();
        if text_markup_should_use_quad_bbox(document, dict, subtype_name) {
            if let Some(points) = quad_points(document, dict) {
                if subtype_name == "Squiggly" {
                    if let Some(box_rect) = squiggly_quad_bbox(&points) {
                        rect = Some(box_rect);
                    }
                } else if let Some(quad_box) = vertices_bbox(&points, 0.0) {
                    rect = Some(quad_box);
                }
            }
        }
    }
    rect
}

/// pdf.js TextAnnotation DEFAULT_ICON_SIZE.
const TEXT_ANNOTATION_ICON_SIZE: f64 = 22.0;

fn normalize_rect_coords(rect: [f64; 4]) -> [f64; 4] {
    let left = rect[0].min(rect[2]);
    let right = rect[0].max(rect[2]);
    let bottom = rect[1].min(rect[3]);
    let top = rect[1].max(rect[3]);
    [left, bottom, right, top]
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

fn vertices_bbox(points: &[(f64, f64)], pad: f64) -> Option<[f64; 4]> {
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

fn box_values(document: &Document, value: &Object) -> Option<[f64; 4]> {
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

pub(super) fn page_box_values(document: &Document, value: &Object) -> Option<[f64; 4]> {
    let values = resolve(document, value).ok()?.as_array().ok()?;
    if values.len() != 4 {
        return None;
    }
    let value = normalize_box(box_values(document, value)?);
    (value[0] < value[2] && value[1] < value[3]).then_some(value)
}

pub(super) fn intersect_boxes(a: [f64; 4], b: [f64; 4]) -> Option<[f64; 4]> {
    let a = normalize_box(a);
    let b = normalize_box(b);
    let value = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (value[0] < value[2] && value[1] < value[3]).then_some(value)
}

fn normalize_box(value: [f64; 4]) -> [f64; 4] {
    [
        value[0].min(value[2]),
        value[1].min(value[3]),
        value[0].max(value[2]),
        value[1].max(value[3]),
    ]
}
