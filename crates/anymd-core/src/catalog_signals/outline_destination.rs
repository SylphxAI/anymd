use lopdf::{Dictionary, Object};
use serde::Serialize;

use super::walker::{Walker, MAX_STRING_BYTES};

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(super) enum Destination {
    Text(String),
    Parts(Vec<DestinationPart>),
    Null,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(super) enum DestinationPart {
    Integer(i64),
    Real(f64),
    Text(String),
    Name { name: String },
    Reference { num: u32, gen: u16 },
    Null,
}

pub(super) fn outline_gotor_url(walker: &mut Walker<'_>, action: &Dictionary) -> Option<String> {
    let file = action.get(b"F").ok()?;
    let mut base = match walker.resolve_owned(file)? {
        Object::String(_, _) | Object::Name(_) => walker.text(file)?,
        Object::Dictionary(dict) => outline_filespec_filename(walker, &dict)?,
        _ => return None,
    };
    if base.is_empty() {
        return None;
    }
    if let Some(remote) = outline_remote_dest(walker, action) {
        if let Some(hash) = base.find('#') {
            base.truncate(hash);
        }
        base.push('#');
        base.push_str(&remote);
    }
    safe_outline_url(walker, &base)
}

fn outline_filespec_filename(walker: &mut Walker<'_>, dict: &Dictionary) -> Option<String> {
    // pdf.js pickPlatformItem order: UF, F, Unix, Mac, DOS.
    for key in [b"UF".as_slice(), b"F", b"Unix", b"Mac", b"DOS"] {
        if let Ok(value) = dict.get(key) {
            if let Some(name) = walker.text(value).filter(|value| !value.is_empty()) {
                return Some(name);
            }
        }
    }
    None
}

fn outline_remote_dest(walker: &mut Walker<'_>, action: &Dictionary) -> Option<String> {
    let value = action.get(b"D").ok()?;
    match walker.resolve_owned(value)? {
        Object::String(_, _) | Object::Name(_) => walker.text(value),
        Object::Array(_) => {
            let dest = normalized_destination(walker, value)?;
            serde_json::to_string(&dest).ok()
        }
        _ => None,
    }
}

pub(super) fn safe_outline_url(walker: &mut Walker<'_>, value: &str) -> Option<String> {
    let parsed = url::Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https" | "ftp" | "mailto" | "tel") {
        return None;
    }
    let normalized = parsed.to_string();
    if normalized.len() > MAX_STRING_BYTES {
        walker.text_remaining = 0;
        walker.truncated = true;
        return None;
    }
    let expansion = normalized.len().saturating_sub(value.len());
    if expansion > walker.text_remaining {
        walker.text_remaining = 0;
        walker.truncated = true;
        return None;
    }
    walker.text_remaining -= expansion;
    Some(normalized)
}

pub(super) fn normalized_destination(
    walker: &mut Walker<'_>,
    value: &Object,
) -> Option<Destination> {
    let value = walker.resolve_owned(value)?;
    match &value {
        Object::String(_, _) | Object::Name(_) => walker.text(&value).map(Destination::Text),
        Object::Array(parts) if parts.len() <= 8 => {
            let mut output = Vec::new();
            for part in parts {
                output.push(match part {
                    Object::Integer(v) => DestinationPart::Integer(*v),
                    Object::Real(v) if v.is_finite() => DestinationPart::Real(f64::from(*v)),
                    Object::String(_, _) => DestinationPart::Text(walker.text(part)?),
                    Object::Name(_) => DestinationPart::Name {
                        name: walker.text(part)?,
                    },
                    Object::Reference((num, gen)) => DestinationPart::Reference {
                        num: *num,
                        gen: *gen,
                    },
                    Object::Null => DestinationPart::Null,
                    _ => return None,
                });
            }
            Some(Destination::Parts(output))
        }
        _ => None,
    }
}
