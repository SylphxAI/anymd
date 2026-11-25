use std::collections::HashSet;

use lopdf::{Dictionary, Object};

use super::super::walker::Walker;

#[derive(Clone, Copy, Default)]
pub(super) struct Inherited<'a> {
    pub(super) field_type: Option<&'a Object>,
    pub(super) flags: Option<&'a Object>,
    pub(super) value: Option<&'a Object>,
    pub(super) default_value: Option<&'a Object>,
}

/// pdf.js Annotation._constructFieldName: walk Parent chain for T parts.
pub(super) fn construct_field_name<'a>(walker: &mut Walker<'a>, dict: &'a Dictionary) -> String {
    let has_t = dict.get(b"T").is_ok();
    let has_parent = dict.get(b"Parent").is_ok();
    if !has_t && !has_parent {
        return String::new();
    }
    if !has_parent {
        return dict
            .get(b"T")
            .ok()
            .and_then(|value| walker.text(value))
            .unwrap_or_default();
    }
    let mut parts = Vec::new();
    if let Some(partial) = dict.get(b"T").ok().and_then(|value| walker.text(value)) {
        if !partial.is_empty() {
            parts.push(partial);
        }
    }
    let mut current = dict.get(b"Parent").ok();
    let mut seen = HashSet::new();
    while let Some(parent_value) = current {
        if let Ok(parent_id) = parent_value.as_reference() {
            if !seen.insert(parent_id) {
                break;
            }
        }
        let Some(parent_dict) = walker.dict(parent_value) else {
            break;
        };
        if let Some(partial) = parent_dict
            .get(b"T")
            .ok()
            .and_then(|value| walker.text(value))
        {
            if !partial.is_empty() {
                parts.push(partial);
            }
        }
        current = parent_dict.get(b"Parent").ok();
    }
    parts.reverse();
    parts.join(".")
}

/// pdf.js getInheritableProperty: first own-or-Parent-chain value for key.
fn inheritable_property<'a>(
    walker: &mut Walker<'a>,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Object> {
    if let Ok(value) = dict.get(key) {
        return Some(value);
    }
    let mut current = dict.get(b"Parent").ok();
    let mut seen = HashSet::new();
    while let Some(parent_value) = current {
        if let Ok(parent_id) = parent_value.as_reference() {
            if !seen.insert(parent_id) {
                break;
            }
        }
        let Some(parent_dict) = walker.dict(parent_value) else {
            break;
        };
        if let Ok(value) = parent_dict.get(key) {
            return Some(value);
        }
        current = parent_dict.get(b"Parent").ok();
    }
    None
}

pub(super) fn inherited_from_parent_chain<'a>(
    walker: &mut Walker<'a>,
    dict: &'a Dictionary,
) -> Inherited<'a> {
    Inherited {
        field_type: inheritable_property(walker, dict, b"FT"),
        flags: inheritable_property(walker, dict, b"Ff"),
        value: inheritable_property(walker, dict, b"V"),
        default_value: inheritable_property(walker, dict, b"DV"),
    }
}
