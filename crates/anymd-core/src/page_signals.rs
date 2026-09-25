use std::collections::HashSet;

use crate::pdfjs_text::decode_pdfjs_text_string;
use lopdf::{Document, Object, ObjectId};
use serde_json::{json, Value};
use url::Url;

const MAX_PARENT_DEPTH: usize = 64;
const MAX_ANNOTATIONS_PER_PAGE: usize = 1_000;
const MAX_ANNOTATIONS_PER_SOURCE: usize = 10_000;
const MAX_STRING_BYTES: usize = 64 * 1024;
const MAX_SIGNAL_TEXT_BYTES: usize = 2 * 1024 * 1024;

struct SignalTextBudget {
    remaining: usize,
    truncated: bool,
    #[cfg(test)]
    decode_attempts: usize,
}

impl SignalTextBudget {
    fn new() -> Self {
        Self {
            remaining: MAX_SIGNAL_TEXT_BYTES,
            truncated: false,
            #[cfg(test)]
            decode_attempts: 0,
        }
    }

    fn admit_raw(&mut self, bytes: usize) -> bool {
        if self.remaining == 0 || bytes > self.remaining {
            self.remaining = 0;
            self.truncated = true;
            false
        } else {
            true
        }
    }

    fn consume(&mut self, bytes: usize) -> bool {
        if bytes > self.remaining {
            self.remaining = 0;
            self.truncated = true;
            false
        } else {
            self.remaining -= bytes;
            true
        }
    }

    fn reject_oversized(&mut self) {
        self.truncated = true;
    }
}

#[derive(Default)]
pub(crate) struct PageSignals {
    pub geometry: Vec<Value>,
    pub annotations: Vec<Value>,
    pub warnings: Vec<String>,
}

pub(crate) fn extract_page_signals(
    document: &Document,
    pages: &[(u32, ObjectId)],
    selected_pages: &[u32],
    want_geometry: bool,
    want_annotations: bool,
) -> PageSignals {
    let mut signals = PageSignals::default();
    let selected: HashSet<u32> = selected_pages.iter().copied().collect();
    let mut annotation_work = 0usize;
    let mut text_budget = SignalTextBudget::new();
    for (page, page_id) in pages {
        if !selected.contains(page) {
            continue;
        }
        if want_geometry {
            if let Some(value) = page_geometry(document, *page, *page_id) {
                signals.geometry.push(value);
            }
        }
        if want_annotations && annotation_work < MAX_ANNOTATIONS_PER_SOURCE {
            let (annotations, truncated, inspected) = page_annotations(
                document,
                *page,
                *page_id,
                (MAX_ANNOTATIONS_PER_SOURCE - annotation_work).min(MAX_ANNOTATIONS_PER_PAGE),
                &mut text_budget,
            );
            annotation_work += inspected;
            if !annotations.is_empty() {
                signals.annotations.push(json!({
                    "page": page,
                    "annotations": annotations,
                }));
            }
            if truncated {
                signals.warnings.push(format!(
                    "include_annotations: page {page} exceeded the bounded annotation limit."
                ));
            }
        }
    }
    if want_annotations && annotation_work >= MAX_ANNOTATIONS_PER_SOURCE {
        signals.warnings.push(format!(
            "include_annotations: source reached the {MAX_ANNOTATIONS_PER_SOURCE} annotation work limit."
        ));
    }
    if want_annotations && text_budget.truncated {
        signals.warnings.push(format!(
            "include_annotations: annotation strings exceeded the {MAX_STRING_BYTES}-byte field or {MAX_SIGNAL_TEXT_BYTES}-byte source text limit."
        ));
    }
    signals
}

fn page_geometry(document: &Document, page: u32, page_id: ObjectId) -> Option<Value> {
    let media = inherited_array(document, page_id, b"MediaBox")
        .and_then(|value| page_box_values(document, value))
        .unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let crop = inherited_array(document, page_id, b"CropBox")
        .and_then(|value| page_box_values(document, value))
        .and_then(|crop| intersect_boxes(media, crop))
        .unwrap_or(media);
    let raw_rotation = inherited_number(document, page_id, b"Rotate").unwrap_or(0.0);
    let rotation = if raw_rotation.is_finite() && raw_rotation.rem_euclid(90.0) == 0.0 {
        raw_rotation.rem_euclid(360.0)
    } else {
        0.0
    };
    // PDF.js exposes page.userUnit from the page dictionary itself; unlike
    // MediaBox/CropBox/Rotate, a Pages-node UserUnit is not inherited there.
    let raw_user_unit = page_number(document, page_id, b"UserUnit").unwrap_or(1.0);
    let user_unit = if raw_user_unit.is_finite() && raw_user_unit > 0.0 {
        raw_user_unit
    } else {
        1.0
    };
    let base_width = (crop[2] - crop[0]).abs() * user_unit;
    let base_height = (crop[3] - crop[1]).abs() * user_unit;
    let quarter_turn =
        (rotation - 90.0).abs() < f64::EPSILON || (rotation - 270.0).abs() < f64::EPSILON;
    let (width, height) = if quarter_turn {
        (base_height, base_width)
    } else {
        (base_width, base_height)
    };
    if !width.is_finite() || !height.is_finite() {
        return None;
    }
    Some(json!({
        "page": page,
        "width": width,
        "height": height,
        "rotation": rotation,
        "user_unit": user_unit,
        "view_box": {
            "left": crop[0], "bottom": crop[1], "right": crop[2], "top": crop[3],
        },
    }))
}

fn page_annotations(
    document: &Document,
    page: u32,
    page_id: ObjectId,
    limit: usize,
    text_budget: &mut SignalTextBudget,
) -> (Vec<Value>, bool, usize) {
    let Some(annots) = inherited_array(document, page_id, b"Annots") else {
        return (Vec::new(), false, 0);
    };
    let Ok(values) = resolve(document, annots).and_then(Object::as_array) else {
        return (Vec::new(), false, 0);
    };
    let truncated = values.len() > limit;
    let inspected = values.len().min(limit);
    let annotations = values
        .iter()
        .take(limit)
        .filter_map(|value| normalize_annotation(document, page, value, text_budget))
        .collect();
    (annotations, truncated, inspected)
}

/// pdf.js TextAnnotation DEFAULT_ICON_SIZE.
const TEXT_ANNOTATION_ICON_SIZE: f64 = 22.0;

fn text_annotation_has_appearance(document: &Document, dict: &lopdf::Dictionary) -> bool {
    let Ok(ap) = dict.get(b"AP") else {
        return false;
    };
    let Ok(ap_dict) = resolve(document, ap).and_then(Object::as_dict) else {
        return false;
    };
    let Ok(normal) = ap_dict.get(b"N") else {
        return false;
    };
    // pdf.js Annotation.setAppearance:
    // - AP/N BaseStream (including empty) sets this.appearance
    // - AP/N named-state dict requires AS name and a stream for that state
    match resolve(document, normal) {
        Ok(Object::Stream(_)) => true,
        Ok(Object::Dictionary(states)) => {
            let Ok(as_name) = dict
                .get(b"AS")
                .and_then(|value| value.as_name().map(|name| name.to_vec()))
            else {
                return false;
            };
            let Ok(selected) = states.get(as_name.as_slice()) else {
                return false;
            };
            resolve(document, selected)
                .ok()
                .is_some_and(|obj| matches!(obj, Object::Stream(_)))
        }
        _ => false,
    }
}

fn popup_parent_dict<'a>(
    document: &'a Document,
    dict: &'a lopdf::Dictionary,
) -> Option<&'a lopdf::Dictionary> {
    let mut parent = dict
        .get(b"Parent")
        .ok()
        .and_then(|value| resolve(document, value).ok())
        .and_then(|value| value.as_dict().ok())?;
    // pdf.js: if Parent.RT == Group, follow IRT for title/contents/color source.
    if parent
        .get(b"RT")
        .ok()
        .and_then(|value| value.as_name().ok())
        .is_some_and(|name| name == b"Group")
    {
        if let Some(irt) = parent
            .get(b"IRT")
            .ok()
            .and_then(|value| resolve(document, value).ok())
            .and_then(|value| value.as_dict().ok())
        {
            parent = irt;
        }
    }
    Some(parent)
}

fn object_number(document: &Document, value: &Object) -> Option<f64> {
    match resolve(document, value).ok()? {
        Object::Integer(v) => Some(*v as f64),
        Object::Real(v) => Some(f64::from(*v)),
        _ => None,
    }
}

fn border_style_width(
    document: &Document,
    dict: &lopdf::Dictionary,
    rect: Option<[f64; 4]>,
) -> f64 {
    // pdf.js Annotation.setBorderStyle:
    // - prefer BS dict (Type absent or Border) width W
    // - else Border array with length >= 3 uses array[2]
    // - else Border short array / missing => width 0
    // Drawing paths then use `width || 1`.
    // When W exceeds half of either Rect dimension (both dimensions > 0), pdf.js clamps to 1.
    // pdf.js: if the BS key is present, never fall through to Border — even when
    // BS is null/non-dict or has a non-Border Type. Default AnnotationBorderStyle
    // width is 1; drawing paths still apply `width || 1`.
    let raw = if dict.get(b"BS").is_ok() {
        if let Some(bs) = dict
            .get(b"BS")
            .ok()
            .and_then(|value| resolve(document, value).ok())
            .and_then(|value| value.as_dict().ok())
        {
            let type_ok = match bs.get(b"Type").ok() {
                None => true,
                Some(value) => value.as_name().ok().is_some_and(|name| name == b"Border"),
            };
            if type_ok {
                bs.get(b"W")
                    .ok()
                    .and_then(|value| object_number(document, value))
                    .unwrap_or(1.0)
            } else {
                1.0
            }
        } else {
            1.0
        }
    } else if let Some(array) = dict
        .get(b"Border")
        .ok()
        .and_then(|value| resolve(document, value).ok())
        .and_then(|value| value.as_array().ok())
    {
        if array.len() >= 3 {
            object_number(document, &array[2]).unwrap_or(0.0)
        } else {
            0.0
        }
    } else {
        // pdf.js sets width 0 when neither BS nor Border is present; drawing uses || 1.
        0.0
    };
    let mut width = if raw == 0.0 { 1.0 } else { raw };
    if width > 0.0 {
        if let Some(rect) = rect {
            let rect = normalize_rect_coords(rect);
            let max_width = (rect[2] - rect[0]) / 2.0;
            let max_height = (rect[3] - rect[1]) / 2.0;
            if max_width > 0.0 && max_height > 0.0 && (width > max_width || width > max_height) {
                width = 1.0;
            }
        }
    }
    width
}

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
    if points.len() < 4 || points.len() % 4 != 0 {
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

fn appearance_resources_have_ext_gstate(document: &Document, dict: &lopdf::Dictionary) -> bool {
    // Highlight keeps built-in appearance only when AP/N stream Resources has ExtGState.
    let Ok(ap) = dict.get(b"AP") else {
        return false;
    };
    let Ok(ap_dict) = resolve(document, ap).and_then(Object::as_dict) else {
        return false;
    };
    let Ok(normal) = ap_dict.get(b"N") else {
        return false;
    };
    let stream = match resolve(document, normal) {
        Ok(Object::Stream(stream)) => stream,
        Ok(Object::Dictionary(states)) => {
            let Ok(as_name) = dict
                .get(b"AS")
                .and_then(|value| value.as_name().map(|name| name.to_vec()))
            else {
                return false;
            };
            let Ok(selected) = states.get(as_name.as_slice()) else {
                return false;
            };
            match resolve(document, selected) {
                Ok(Object::Stream(stream)) => stream,
                _ => return false,
            }
        }
        _ => return false,
    };
    let Ok(resources) = stream.dict.get(b"Resources") else {
        return false;
    };
    let Ok(resources_dict) = resolve(document, resources).and_then(Object::as_dict) else {
        return false;
    };
    resources_dict.get(b"ExtGState").is_ok()
}

fn text_markup_should_use_quad_bbox(
    document: &Document,
    dict: &lopdf::Dictionary,
    subtype: &str,
) -> bool {
    // pdf.js text-markup annotations:
    // - Underline/Squiggly/StrikeOut: synthesize from QuadPoints only when appearance is unset
    // - Highlight: also ignore appearance streams whose Resources lack ExtGState
    match subtype {
        "Highlight" => {
            !annotation_has_normal_appearance(document, dict)
                || !appearance_resources_have_ext_gstate(document, dict)
        }
        "Underline" | "Squiggly" | "StrikeOut" => !annotation_has_normal_appearance(document, dict),
        _ => false,
    }
}

fn annotation_has_normal_appearance(document: &Document, dict: &lopdf::Dictionary) -> bool {
    // pdf.js Annotation.setAppearance: AP/N must be a stream, or a named-state
    // dict with AS selecting a stream. A bare AP/N key (null/name/non-stream)
    // does not set appearance, so Line/PolyLine/Ink keep geometry expansion.
    // Share the same gate as Text annotation appearance detection.
    text_annotation_has_appearance(document, dict)
}

/// pdf.js Catalog.parseDestDictionary + createValidAbsoluteUrl:
/// public `url` prefers absoluteUrl.href when the raw URI parses as a valid
/// absolute URL (with optional www. → http:// default protocol). Otherwise the
/// raw string is kept (TS falls back to unsafeUrl).
fn normalize_public_annotation_url(raw: String) -> String {
    let candidate = if raw.starts_with("www.") {
        let dots = raw.bytes().filter(|b| *b == b'.').count();
        if dots >= 2 {
            format!("http://{raw}")
        } else {
            raw.clone()
        }
    } else {
        raw.clone()
    };
    match Url::parse(&candidate) {
        Ok(url) if matches!(url.scheme(), "http" | "https" | "ftp" | "mailto" | "tel") => {
            url.to_string()
        }
        _ => raw,
    }
}

fn normalize_annotation(
    document: &Document,
    page: u32,
    value: &Object,
    text_budget: &mut SignalTextBudget,
) -> Option<Value> {
    let id = match value {
        Object::Reference((object, generation)) => Some(if *generation == 0 {
            format!("{object}R")
        } else {
            format!("{object}R{generation}")
        }),
        _ => None,
    };
    let dict = resolve(document, value).ok()?.as_dict().ok()?;
    let subtype = bounded_name(dict.get(b"Subtype").ok()?, text_budget);
    let mut contents = dict
        .get(b"Contents")
        .ok()
        .and_then(|value| decoded_string(document, value, text_budget));
    let mut title = dict
        .get(b"T")
        .ok()
        .and_then(|value| decoded_string(document, value, text_budget));
    // pdf.js MarkupAnnotation: when RT == Group, title/contents are taken from IRT
    // (overwriting local values). Public TS projection only surfaces those fields.
    if dict
        .get(b"RT")
        .ok()
        .and_then(|value| value.as_name().ok())
        .is_some_and(|name| name == b"Group")
    {
        if let Some(irt) = dict
            .get(b"IRT")
            .ok()
            .and_then(|value| resolve(document, value).ok())
            .and_then(|value| value.as_dict().ok())
        {
            title = irt
                .get(b"T")
                .ok()
                .and_then(|value| decoded_string(document, value, text_budget));
            contents = irt
                .get(b"Contents")
                .ok()
                .and_then(|value| decoded_string(document, value, text_budget));
        }
    }
    // pdf.js WidgetAnnotation stores /T as fieldName, not titleObj. Public TS
    // normalizeAnnotation only projects title from titleObj, so Widget omits title.
    if subtype.as_deref() == Some("Widget") {
        title = None;
    }
    // pdf.js PopupAnnotation always projects title/contents from Parent (and IRT
    // when Parent.RT is Group). Public TS projection only surfaces title/contents.
    if subtype.as_deref() == Some("Popup") {
        if let Some(parent) = popup_parent_dict(document, dict) {
            title = parent
                .get(b"T")
                .ok()
                .and_then(|value| decoded_string(document, value, text_budget));
            contents = parent
                .get(b"Contents")
                .ok()
                .and_then(|value| decoded_string(document, value, text_budget));
        }
    }
    let mut rect = dict
        .get(b"Rect")
        .ok()
        .and_then(|value| box_values(document, value));
    // pdf.js normalizes Rect first (lookupNormalRect), then TextAnnotation without
    // appearance forces a 22x22 icon box anchored at the top-left of that box:
    // bottom = top - 22, right = left + 22.
    if subtype.as_deref() == Some("Text") && !text_annotation_has_appearance(document, dict) {
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
    if subtype.as_deref() == Some("Popup") {
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
    if subtype.as_deref() == Some("Line") && !annotation_has_normal_appearance(document, dict) {
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
    if matches!(subtype.as_deref(), Some("PolyLine") | Some("Polygon"))
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
    if subtype.as_deref() == Some("Ink") && !annotation_has_normal_appearance(document, dict) {
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
        subtype.as_deref(),
        Some("Highlight") | Some("Underline") | Some("Squiggly") | Some("StrikeOut")
    ) {
        let subtype_name = subtype.as_deref().unwrap();
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

    let direct_dest = dict
        .get(b"Dest")
        .ok()
        .and_then(|value| destination(document, value, text_budget));
    // pdf.js Catalog.parseDestDictionary:
    // 1) prefer /A action dict
    // 2) else if /Dest present, treat Dest as the action object (handled via direct_dest)
    // 3) else fall back to /AA additional-actions, preferring /D then /U
    let action = dict
        .get(b"A")
        .ok()
        .and_then(|value| resolve(document, value).ok())
        .and_then(|value| value.as_dict().ok())
        .or_else(|| {
            if dict.get(b"Dest").is_ok() {
                return None;
            }
            let aa = dict
                .get(b"AA")
                .ok()
                .and_then(|value| resolve(document, value).ok())
                .and_then(|value| value.as_dict().ok())?;
            aa.get(b"D")
                .ok()
                .or_else(|| aa.get(b"U").ok())
                .and_then(|value| resolve(document, value).ok())
                .and_then(|value| value.as_dict().ok())
        });
    let action_kind = action
        .as_ref()
        .and_then(|action| action.get(b"S").ok())
        .and_then(|value| value.as_name().ok());
    // pdf.js Link annotations project destination/url from the action when
    // present: GoTo supplies dest (winning over /Dest), URI/Launch/GoToR supply
    // url and suppress dest, and only action-less annotations fall back to /Dest.
    // Launch/GoToR file specs prefer UF over F (pdf.js FileSpec/pickPlatformItem).
    // GoToR appends `#` + remote dest (string name or JSON explicit dest).
    // AA/D and AA/U are admitted only when /A is absent and /Dest is absent.
    let (dest, url) = match action_kind {
        Some(b"GoTo") => {
            let dest = action
                .as_ref()
                .and_then(|action| action.get(b"D").ok())
                .and_then(|value| destination(document, value, text_budget));
            (dest, None)
        }
        Some(b"URI") => {
            // pdf.js parseDestDictionary: if URI is a Name, url = "/" + name.
            let url = action.as_ref().and_then(|action| {
                let value = action.get(b"URI").ok()?;
                match resolve(document, value).ok()? {
                    Object::Name(name) => {
                        let text = std::str::from_utf8(name).ok()?.to_string();
                        if text.is_empty() {
                            return None;
                        }
                        Some(format!("/{text}"))
                    }
                    _ => decoded_string(document, value, text_budget),
                }
            });
            (None, url)
        }
        Some(b"Launch") | Some(b"GoToR") => {
            let url = action.as_ref().and_then(|action| {
                let file = action.get(b"F").ok()?;
                let mut base = match resolve(document, file).ok()? {
                    Object::String(_, _) => decoded_string(document, file, text_budget)?,
                    Object::Dictionary(ref dict) => {
                        filespec_raw_filename(document, dict, text_budget)?
                    }
                    Object::Name(_) => bounded_name(file, text_budget)?,
                    _ => return None,
                };
                if action_kind == Some(b"GoToR") {
                    if let Some(remote) = fetch_remote_dest(document, action, text_budget) {
                        if let Some(hash) = base.find('#') {
                            base.truncate(hash);
                        }
                        base.push('#');
                        base.push_str(&remote);
                    }
                }
                Some(base)
            });
            (None, url)
        }
        _ => (direct_dest, None),
    };

    if id.is_none()
        && subtype.is_none()
        && contents.is_none()
        && title.is_none()
        && url.is_none()
        && dest.is_none()
    {
        return None;
    }
    let mut output = serde_json::Map::new();
    output.insert("page".into(), json!(page));
    if let Some(value) = id {
        output.insert("id".into(), json!(value));
    }
    if let Some(value) = subtype.filter(|value| !value.trim().is_empty()) {
        output.insert("subtype".into(), json!(value));
    }
    if let Some(value) = contents.filter(|value| !value.trim().is_empty()) {
        output.insert("contents".into(), json!(value));
    }
    if let Some(value) = title.filter(|value| !value.trim().is_empty()) {
        output.insert("title".into(), json!(value));
    }
    if let Some(value) = url.filter(|value| !value.is_empty()) {
        output.insert("url".into(), json!(normalize_public_annotation_url(value)));
    }
    if let Some(value) = dest {
        output.insert("dest".into(), value);
    }
    if let Some(value) = rect {
        output.insert(
            "bounding_box".into(),
            json!({
                "left": value[0].min(value[2]), "bottom": value[1].min(value[3]),
                "right": value[0].max(value[2]), "top": value[1].max(value[3]),
            }),
        );
    }
    Some(Value::Object(output))
}

fn inherited_array<'a>(
    document: &'a Document,
    page_id: ObjectId,
    key: &[u8],
) -> Option<&'a Object> {
    inherited(document, page_id, key)
}

fn inherited_number(document: &Document, page_id: ObjectId, key: &[u8]) -> Option<f64> {
    let value = inherited(document, page_id, key)?;
    number(resolve(document, value).ok()?)
}

fn page_number(document: &Document, page_id: ObjectId, key: &[u8]) -> Option<f64> {
    let dict = document.get_object(page_id).ok()?.as_dict().ok()?;
    number(resolve(document, dict.get(key).ok()?).ok()?)
}

fn inherited<'a>(document: &'a Document, mut id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut visited = HashSet::new();
    for _ in 0..MAX_PARENT_DEPTH {
        if !visited.insert(id) {
            return None;
        }
        let dict = document.get_object(id).ok()?.as_dict().ok()?;
        if let Ok(value) = dict.get(key) {
            return Some(value);
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn resolve<'a>(document: &'a Document, value: &'a Object) -> Result<&'a Object, lopdf::Error> {
    match value {
        Object::Reference(id) => document.get_object(*id),
        _ => Ok(value),
    }
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

fn page_box_values(document: &Document, value: &Object) -> Option<[f64; 4]> {
    let values = resolve(document, value).ok()?.as_array().ok()?;
    if values.len() != 4 {
        return None;
    }
    let value = normalize_box(box_values(document, value)?);
    (value[0] < value[2] && value[1] < value[3]).then_some(value)
}

fn number(value: &Object) -> Option<f64> {
    match value {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

fn intersect_boxes(a: [f64; 4], b: [f64; 4]) -> Option<[f64; 4]> {
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

fn bounded_name(value: &Object, budget: &mut SignalTextBudget) -> Option<String> {
    let bytes = value.as_name().ok()?;
    if bytes.len() > MAX_STRING_BYTES {
        budget.reject_oversized();
        return None;
    }
    if !budget.admit_raw(bytes.len()) {
        return None;
    }
    let decoded = String::from_utf8_lossy(bytes).into_owned();
    if !budget.consume(decoded.len()) {
        return None;
    }
    Some(decoded)
}

fn decoded_string(
    document: &Document,
    value: &Object,
    budget: &mut SignalTextBudget,
) -> Option<String> {
    let value = resolve(document, value).ok()?;
    if let Object::String(bytes, _) = value {
        if bytes.len() > MAX_STRING_BYTES {
            budget.reject_oversized();
            return None;
        }
        if !budget.admit_raw(bytes.len()) {
            return None;
        }
    }
    #[cfg(test)]
    {
        budget.decode_attempts += 1;
    }
    let decoded = decode_pdfjs_text_string(value)?;
    if decoded.len() > MAX_STRING_BYTES {
        budget.reject_oversized();
        return None;
    }
    if !budget.consume(decoded.len()) {
        return None;
    }
    Some(decoded)
}

fn filespec_raw_filename(
    document: &Document,
    dict: &lopdf::Dictionary,
    budget: &mut SignalTextBudget,
) -> Option<String> {
    // pdf.js pickPlatformItem order: UF, F, Unix, Mac, DOS.
    for key in [b"UF".as_slice(), b"F", b"Unix", b"Mac", b"DOS"] {
        if let Ok(value) = dict.get(key) {
            let resolved = match value {
                Object::Reference(_) => resolve(document, value).ok()?,
                other => other,
            };
            let name = match resolved {
                Object::String(_, _) => decoded_string(document, resolved, budget),
                Object::Name(_) => bounded_name(resolved, budget),
                _ => None,
            };
            if let Some(name) = name.filter(|value| !value.is_empty()) {
                return Some(name);
            }
        }
    }
    None
}

fn fetch_remote_dest(
    document: &Document,
    action: &lopdf::Dictionary,
    budget: &mut SignalTextBudget,
) -> Option<String> {
    let value = action.get(b"D").ok()?;
    // Named dest string/name.
    match value {
        Object::String(_, _) => return decoded_string(document, value, budget),
        Object::Name(_) => return bounded_name(value, budget),
        Object::Reference(_) => {
            let resolved = resolve(document, value).ok()?;
            match resolved {
                Object::String(_, _) => return decoded_string(document, resolved, budget),
                Object::Name(_) => return bounded_name(resolved, budget),
                Object::Array(_) => {
                    let projected = destination(document, resolved, budget)?;
                    return Some(projected.to_string());
                }
                _ => return None,
            }
        }
        Object::Array(_) => {
            let projected = destination(document, value, budget)?;
            return Some(projected.to_string());
        }
        _ => {}
    }
    None
}

fn destination(
    document: &Document,
    value: &Object,
    budget: &mut SignalTextBudget,
) -> Option<Value> {
    // Match pdf.js annotation dest shape: keep page object refs as {num,gen}
    // and name tokens as {name}, without resolving array members first.
    let value = match value {
        Object::Reference(_) => resolve(document, value).ok()?,
        other => other,
    };
    match value {
        Object::String(_, _) => decoded_string(document, value, budget).map(Value::String),
        Object::Name(_) => bounded_name(value, budget).map(Value::String),
        Object::Array(values) if values.len() <= 8 => {
            let mut parts = Vec::with_capacity(values.len());
            for part in values {
                let projected = match part {
                    Object::Reference((object, generation)) => {
                        Some(json!({ "num": object, "gen": generation }))
                    }
                    Object::Integer(value) => Some(json!(value)),
                    Object::Real(value) => Some(json!(value)),
                    Object::Name(_) => {
                        bounded_name(part, budget).map(|name| json!({ "name": name }))
                    }
                    Object::String(_, _) => {
                        decoded_string(document, part, budget).map(Value::String)
                    }
                    Object::Null => Some(Value::Null),
                    other => match resolve(document, other).ok()? {
                        Object::Integer(value) => Some(json!(value)),
                        Object::Real(value) => Some(json!(value)),
                        Object::Name(_) => bounded_name(resolve(document, other).ok()?, budget)
                            .map(|name| json!({ "name": name })),
                        Object::String(_, _) => {
                            decoded_string(document, resolve(document, other).ok()?, budget)
                                .map(Value::String)
                        }
                        Object::Null => Some(Value::Null),
                        _ => None,
                    },
                }?;
                parts.push(projected);
            }
            Some(Value::Array(parts))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
