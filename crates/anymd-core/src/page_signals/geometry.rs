use lopdf::{Document, Object};

use super::pdf_objects::resolve;

mod appearance;
mod drawing;
mod markup;
mod notes;
mod rectangles;

use rectangles::{box_values, normalize_rect_coords};

pub(super) fn annotation_rect(
    document: &Document,
    dict: &lopdf::Dictionary,
    subtype: Option<&str>,
) -> Option<[f64; 4]> {
    let rect = dict
        .get(b"Rect")
        .ok()
        .and_then(|value| box_values(document, value));
    match subtype {
        Some("Text") => notes::note_rect(document, dict, rect),
        Some("Popup") => notes::popup_rect(rect),
        Some("Line") => drawing::line_rect(document, dict, rect),
        Some("PolyLine" | "Polygon") => drawing::polygon_rect(document, dict, rect),
        Some("Ink") => drawing::ink_rect(document, dict, rect),
        Some(markup @ ("Highlight" | "Underline" | "Squiggly" | "StrikeOut")) => {
            markup::markup_rect(document, dict, markup, rect)
        }
        _ => rect,
    }
}

pub(super) fn page_box_values(document: &Document, value: &Object) -> Option<[f64; 4]> {
    let values = resolve(document, value).ok()?.as_array().ok()?;
    if values.len() != 4 {
        return None;
    }
    let value = normalize_rect_coords(box_values(document, value)?);
    (value[0] < value[2] && value[1] < value[3]).then_some(value)
}

pub(super) fn intersect_boxes(a: [f64; 4], b: [f64; 4]) -> Option<[f64; 4]> {
    let a = normalize_rect_coords(a);
    let b = normalize_rect_coords(b);
    let value = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (value[0] < value[2] && value[1] < value[3]).then_some(value)
}
