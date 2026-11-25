use std::collections::HashSet;

use lopdf::{Dictionary, Object, ObjectId};

use self::{
    pages::WidgetPages,
    parent_chain::{construct_field_name, inherited_from_parent_chain},
    widget::normalize_leaf,
};
use super::{
    walker::{Walker, MAX_DEPTH, MAX_ENTRIES},
    FormField,
};

mod pages;
mod parent_chain;
mod widget;

pub(super) fn extract_forms<'a>(
    walker: &mut Walker<'a>,
    catalog: &'a Dictionary,
    pages: &[(u32, ObjectId)],
) -> Option<Vec<FormField>> {
    let acroform = walker.dict(catalog.get(b"AcroForm").ok()?)?;
    let fields = walker.array_bounded(acroform.get(b"Fields").ok()?, MAX_ENTRIES)?;
    let pages = WidgetPages::collect(walker, pages)?;
    let mut traversal = FieldTraversal {
        walker,
        pages,
        visited: HashSet::new(),
        output: Vec::new(),
        raw_node_budget: MAX_ENTRIES - fields.len(),
    };
    for field in fields {
        if field.as_reference().is_err() {
            continue;
        }
        traversal.walk_field(field, 0);
        if traversal.walker.failed || traversal.walker.limited {
            return None;
        }
    }
    (!traversal.output.is_empty()).then_some(traversal.output)
}

struct FieldTraversal<'a, 'w> {
    walker: &'w mut Walker<'a>,
    pages: WidgetPages,
    visited: HashSet<ObjectId>,
    output: Vec<FormField>,
    raw_node_budget: usize,
}

impl<'a> FieldTraversal<'a, '_> {
    fn walk_field(&mut self, value: &'a Object, depth: usize) {
        if depth >= MAX_DEPTH || self.output.len() >= MAX_ENTRIES {
            self.walker.limited = true;
            return;
        }
        let Ok(id) = value.as_reference() else {
            return;
        };
        if !self.visited.insert(id) {
            return;
        }
        let Some(resolved) = self.walker.resolve(value) else {
            return;
        };
        let Ok(dict) = resolved.as_dict() else {
            return;
        };
        if dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Link") {
            return;
        }
        let kids = dict
            .get(b"Kids")
            .ok()
            .and_then(|value| self.walker.array_bounded(value, self.raw_node_budget));
        if let Some(kids) = kids.as_ref() {
            self.raw_node_budget -= kids.len();
        }
        // Public names follow pdf.js Parent-chain construction, not Kids-path alone.
        let public_name = construct_field_name(self.walker, dict).trim().to_string();
        if !public_name.is_empty() {
            if kids.is_some() {
                self.output.push(FormField {
                    name: public_name.clone(),
                    r#type: None,
                    value: None,
                    default_value: None,
                    page: None,
                    id: Some(format_id(id)),
                    editable: None,
                    bounding_box: None,
                });
            } else {
                let inherited = inherited_from_parent_chain(self.walker, dict);
                // pdf.js WidgetAnnotation without inheritable FT falls back to base
                // WidgetAnnotation whose getFieldObject() returns null.
                if inherited.field_type.is_some() {
                    if let Some(field) = normalize_leaf(
                        self.walker,
                        dict,
                        Some(id),
                        public_name,
                        &inherited,
                        &self.pages,
                    ) {
                        self.output.push(field);
                    }
                }
            }
        }
        if let Some(kids) = kids {
            for kid in kids {
                self.walk_field(kid, depth + 1);
            }
        }
    }
}

pub(super) fn format_id((num, generation): ObjectId) -> String {
    if generation == 0 {
        format!("{num}R")
    } else {
        format!("{num}R{generation}")
    }
}
