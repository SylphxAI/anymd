use lopdf::{Document, Object};

use super::super::pdf_objects::resolve;
use super::normalize_rect_coords;

pub(super) fn text_annotation_has_appearance(
    document: &Document,
    dict: &lopdf::Dictionary,
) -> bool {
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

fn object_number(document: &Document, value: &Object) -> Option<f64> {
    match resolve(document, value).ok()? {
        Object::Integer(v) => Some(*v as f64),
        Object::Real(v) => Some(f64::from(*v)),
        _ => None,
    }
}

pub(super) fn border_style_width(
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

pub(super) fn text_markup_should_use_quad_bbox(
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

pub(super) fn annotation_has_normal_appearance(
    document: &Document,
    dict: &lopdf::Dictionary,
) -> bool {
    // pdf.js Annotation.setAppearance: AP/N must be a stream, or a named-state
    // dict with AS selecting a stream. A bare AP/N key (null/name/non-stream)
    // does not set appearance, so Line/PolyLine/Ink keep geometry expansion.
    // Share the same gate as Text annotation appearance detection.
    text_annotation_has_appearance(document, dict)
}
