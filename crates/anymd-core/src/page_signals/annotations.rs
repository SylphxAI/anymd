use lopdf::{Document, Object, ObjectId};
use serde_json::{json, Value};
use url::Url;

use super::geometry::annotation_rect;
use super::pdf_objects::{bounded_name, decoded_string, inherited_array, resolve};
use super::SignalTextBudget;

mod actions;

use actions::annotation_action;

pub(super) fn page_annotations(
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
    let rect = annotation_rect(document, dict, subtype.as_deref());
    let (dest, url) = annotation_action(document, dict, text_budget);

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
