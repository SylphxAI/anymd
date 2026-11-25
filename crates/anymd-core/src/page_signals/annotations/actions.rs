use lopdf::{Document, Object};
use serde_json::{json, Value};

use super::super::pdf_objects::{bounded_name, decoded_string, resolve};
use super::super::SignalTextBudget;

pub(super) fn annotation_action(
    document: &Document,
    dict: &lopdf::Dictionary,
    text_budget: &mut SignalTextBudget,
) -> (Option<Value>, Option<String>) {
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
    match action_kind {
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
    }
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
