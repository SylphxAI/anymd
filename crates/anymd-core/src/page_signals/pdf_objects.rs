use std::collections::HashSet;

use lopdf::{Document, Object, ObjectId};

use crate::pdfjs_text::decode_pdfjs_text_string;

use super::{SignalTextBudget, MAX_STRING_BYTES};

const MAX_PARENT_DEPTH: usize = 64;

pub(super) fn inherited_array<'a>(
    document: &'a Document,
    page_id: ObjectId,
    key: &[u8],
) -> Option<&'a Object> {
    inherited(document, page_id, key)
}

pub(super) fn inherited_number(document: &Document, page_id: ObjectId, key: &[u8]) -> Option<f64> {
    let value = inherited(document, page_id, key)?;
    number(resolve(document, value).ok()?)
}

pub(super) fn page_number(document: &Document, page_id: ObjectId, key: &[u8]) -> Option<f64> {
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

pub(super) fn resolve<'a>(
    document: &'a Document,
    value: &'a Object,
) -> Result<&'a Object, lopdf::Error> {
    match value {
        Object::Reference(id) => document.get_object(*id),
        _ => Ok(value),
    }
}

pub(super) fn number(value: &Object) -> Option<f64> {
    match value {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

pub(super) fn bounded_name(value: &Object, budget: &mut SignalTextBudget) -> Option<String> {
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

pub(super) fn decoded_string(
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
