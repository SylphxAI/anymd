use std::collections::HashSet;

use lopdf::{Document, ObjectId};
use serde_json::{json, Value};

mod annotations;
mod geometry;
mod pdf_objects;

use annotations::page_annotations;
use geometry::{intersect_boxes, page_box_values};
use pdf_objects::{inherited_array, inherited_number, page_number};

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

#[cfg(test)]
mod tests;
