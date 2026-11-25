use lopdf::{Dictionary, Object, ObjectId};
use serde_json::Value;

use self::buttons::normalize_button;
use super::super::{
    walker::{Walker, MAX_DEPTH},
    BoxValue, FormField,
};
use super::{format_id, pages::WidgetPages, parent_chain::Inherited};

mod buttons;

pub(super) fn normalize_leaf<'a>(
    walker: &mut Walker<'a>,
    dict: &'a Dictionary,
    id: Option<ObjectId>,
    name: String,
    inherited: &Inherited<'a>,
    pages: &WidgetPages,
) -> Option<FormField> {
    let ft = inherited
        .field_type
        .and_then(|v| walker.resolve(v))
        .and_then(|v| v.as_name().ok().map(<[u8]>::to_vec));
    let flags = inherited
        .flags
        .and_then(|v| walker.resolve(v))
        .and_then(|v| v.as_i64().ok())
        .unwrap_or(0);
    let field_type = match ft.as_deref() {
        Some(b"Tx") => Some("text"),
        Some(b"Btn") if flags & (1 << 16) != 0 => Some("button"),
        Some(b"Btn") if flags & (1 << 15) != 0 => Some("radiobutton"),
        Some(b"Btn") => Some("checkbox"),
        Some(b"Ch") if flags & (1 << 17) != 0 => Some("combobox"),
        Some(b"Ch") => Some("listbox"),
        Some(b"Sig") => Some("signature"),
        _ => None,
    }
    .map(str::to_string);
    let raw_value = inherited.value.map(|v| form_value(walker, v));
    let mut default_value = inherited.default_value.and_then(|v| form_value(walker, v));
    let fallback = default_value.clone().filter(|value| !value.is_null());
    let mut value = raw_value.unwrap_or(fallback);
    match field_type.as_deref() {
        Some("text") => {
            value = Some(match value {
                Some(Value::String(value)) => Value::String(value),
                _ => Value::String(String::new()),
            });
            if default_value.as_ref().is_none_or(Value::is_null) {
                default_value = Some(Value::String(String::new()));
            }
        }
        Some("checkbox" | "radiobutton" | "button") => {
            normalize_button(
                walker,
                dict,
                field_type.as_deref(),
                &mut value,
                &mut default_value,
            );
        }
        Some("listbox" | "combobox") => {
            value = Some(first_choice_value(value));
            default_value.get_or_insert(Value::Null);
        }
        Some("signature") => {
            value = Some(Value::Null);
            default_value = None;
        }
        _ => {}
    }
    let page = pages.page_of(dict, id);
    let bounding_box = dict.get(b"Rect").ok().and_then(|v| rect(walker, v));
    Some(FormField {
        name,
        r#type: field_type.clone(),
        value,
        default_value,
        page,
        id: id.map(format_id),
        editable: field_type
            .as_deref()
            .filter(|kind| *kind != "signature")
            .map(|_| flags & 1 == 0),
        bounding_box: (field_type.as_deref() != Some("signature"))
            .then_some(bounding_box)
            .flatten(),
    })
}

fn first_choice_value(value: Option<Value>) -> Value {
    match value {
        Some(Value::Array(values)) => values.into_iter().next().unwrap_or(Value::Null),
        Some(value) => value,
        None => Value::Null,
    }
}

fn form_value<'a>(walker: &mut Walker<'a>, value: &'a Object) -> Option<Value> {
    form_value_at_depth(walker, value, 0)
}

fn form_value_at_depth<'a>(
    walker: &mut Walker<'a>,
    value: &'a Object,
    depth: usize,
) -> Option<Value> {
    if depth >= MAX_DEPTH || walker.form_value_nodes_remaining == 0 {
        walker.limited = true;
        return None;
    }
    walker.form_value_nodes_remaining -= 1;
    let value = walker.resolve(value)?;
    match value {
        Object::String(_, _) | Object::Name(_) => walker.text(value).map(Value::String),
        Object::Array(values) => {
            if values.len() > 256 || values.len() > walker.form_value_nodes_remaining {
                walker.limited = true;
                return None;
            }
            let mut decoded = Vec::new();
            for value in values {
                if let Some(value) = form_value_at_depth(walker, value, depth + 1) {
                    if !value.is_null() {
                        decoded.push(value);
                    }
                }
                if walker.limited {
                    return None;
                }
            }
            Some(if decoded.is_empty() {
                Value::Null
            } else {
                Value::Array(decoded)
            })
        }
        _ => Some(Value::Null),
    }
}

fn rect<'a>(walker: &mut Walker<'a>, value: &'a Object) -> Option<BoxValue> {
    let value = walker.resolve(value)?;
    let values = value.as_array().ok()?;
    if values.len() < 4 {
        return None;
    }
    let mut n = [0.0; 4];
    for (index, value) in values.iter().take(4).enumerate() {
        n[index] = number(walker.resolve(value)?)?;
    }
    Some(BoxValue {
        left: n[0].min(n[2]),
        bottom: n[1].min(n[3]),
        right: n[0].max(n[2]),
        top: n[1].max(n[3]),
    })
}

fn number(value: &Object) -> Option<f64> {
    let n = match value {
        Object::Integer(v) => *v as f64,
        Object::Real(v) => f64::from(*v),
        _ => return None,
    };
    n.is_finite().then_some(n)
}
