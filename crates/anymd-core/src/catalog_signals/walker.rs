use std::collections::HashSet;

use lopdf::{Dictionary, Document, Object};
use serde::Serialize;

use crate::cos_document::EncryptionFacts;
use crate::pdfjs_text::decode_pdfjs_text_string;

const MAX_REFERENCE_DEPTH: usize = 64;
pub(super) const MAX_TREE_DEPTH: usize = 64;
const MAX_OBJECT_WORK: usize = 100_000;
pub(super) const MAX_ENTRIES: usize = 10_000;
pub(super) const MAX_STRING_BYTES: usize = 64 * 1024;
pub(super) const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct MarkInfo {
    #[serde(rename = "Marked")]
    marked: bool,
    #[serde(rename = "UserProperties")]
    user_properties: bool,
    #[serde(rename = "Suspects")]
    suspects: bool,
}

pub(super) struct Walker<'a> {
    document: &'a Document,
    work: usize,
    pub(super) entries: usize,
    pub(super) text_remaining: usize,
    pub(super) truncated: bool,
    pub(super) failed: bool,
}

impl<'a> Walker<'a> {
    pub(super) fn new(document: &'a Document) -> Self {
        Self {
            document,
            work: 0,
            entries: 0,
            text_remaining: MAX_TEXT_BYTES,
            truncated: false,
            failed: false,
        }
    }

    pub(super) fn resolve_owned(&mut self, value: &Object) -> Option<Object> {
        let mut current = value.clone();
        let mut visited = HashSet::new();
        for _ in 0..MAX_REFERENCE_DEPTH {
            self.work += 1;
            if self.work > MAX_OBJECT_WORK {
                self.truncated = true;
                return None;
            }
            let Object::Reference(id) = current else {
                return Some(current);
            };
            if !visited.insert(id) {
                self.truncated = true;
                return None;
            }
            let Ok(next) = self.document.get_object(id) else {
                self.failed = true;
                return None;
            };
            current = next.clone();
        }
        self.truncated = true;
        None
    }

    pub(super) fn dict(&mut self, value: &Object) -> Option<Dictionary> {
        self.resolve_owned(value)?.as_dict().ok().cloned()
    }

    pub(super) fn text(&mut self, value: &Object) -> Option<String> {
        let value = self.resolve_owned(value)?;
        let raw_len = match &value {
            Object::String(bytes, _) | Object::Name(bytes) => bytes.len(),
            _ => return None,
        };
        if raw_len > MAX_STRING_BYTES || raw_len > self.text_remaining {
            self.text_remaining = 0;
            self.truncated = true;
            return None;
        }
        let decoded = match &value {
            Object::Name(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => match decode_pdfjs_text_string(&value) {
                Some(value) => value,
                None => {
                    self.failed = true;
                    return None;
                }
            },
        };
        if decoded.len() > MAX_STRING_BYTES || decoded.len() > self.text_remaining {
            self.text_remaining = 0;
            self.truncated = true;
            return None;
        }
        self.text_remaining -= decoded.len();
        Some(decoded)
    }
}

pub(super) fn extract_mark_info(walker: &mut Walker<'_>, catalog: &Dictionary) -> Option<MarkInfo> {
    let dict = walker.dict(catalog.get(b"MarkInfo").ok()?)?;
    let mut boolean = |key: &[u8]| {
        dict.get(key)
            .ok()
            .and_then(|value| walker.resolve_owned(value))
            .and_then(|value| value.as_bool().ok())
    };
    let value = MarkInfo {
        marked: boolean(b"Marked").unwrap_or(false),
        user_properties: boolean(b"UserProperties").unwrap_or(false),
        suspects: boolean(b"Suspects").unwrap_or(false),
    };
    (!walker.truncated && !walker.failed).then_some(value)
}

pub(super) fn extract_permissions(facts: Option<EncryptionFacts>) -> Option<Vec<String>> {
    let facts = facts?;
    let raw = facts.permissions?;
    let bits = (raw as i32) as u32;
    let mut labels = Vec::new();
    for (flag, label) in [
        (4, "print"),
        (8, "modify"),
        (16, "copy"),
        (32, "annotate"),
        (256, "fill_forms"),
        (512, "copy_for_accessibility"),
        (1024, "assemble"),
        (2048, "print_high_quality"),
    ] {
        if bits & flag != 0 {
            labels.push(label.to_string());
        }
    }
    (!labels.is_empty()).then_some(labels)
}
