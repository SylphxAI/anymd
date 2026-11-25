use std::collections::HashSet;

use lopdf::{Dictionary, Document, Object};

use crate::pdfjs_text::decode_pdfjs_text_string;

pub(super) const MAX_DEPTH: usize = 64;
const MAX_OBJECTS: usize = 100_000;
pub(super) const MAX_ENTRIES: usize = 10_000;
const MAX_STRING_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

pub(super) struct Walker<'a> {
    pub(super) document: &'a Document,
    work: usize,
    pub(super) entries: usize,
    text_remaining: usize,
    pub(super) form_value_nodes_remaining: usize,
    pub(super) failed: bool,
    pub(super) limited: bool,
    #[cfg(test)]
    pub(super) materialized_array_items: usize,
    #[cfg(test)]
    pub(super) annotation_materialized_array_items: usize,
}

impl<'a> Walker<'a> {
    pub(super) fn new(document: &'a Document) -> Self {
        Self {
            document,
            work: 0,
            entries: 0,
            text_remaining: MAX_TEXT_BYTES,
            form_value_nodes_remaining: MAX_ENTRIES,
            failed: false,
            limited: false,
            #[cfg(test)]
            materialized_array_items: 0,
            #[cfg(test)]
            annotation_materialized_array_items: 0,
        }
    }

    pub(super) fn resolve(&mut self, mut value: &'a Object) -> Option<&'a Object> {
        let mut seen = HashSet::new();
        for _ in 0..MAX_DEPTH {
            self.work += 1;
            if self.work > MAX_OBJECTS {
                self.limited = true;
                return None;
            }
            let Object::Reference(id) = value else {
                return Some(value);
            };
            if !seen.insert(*id) {
                self.failed = true;
                return None;
            }
            let Some(next) = self.document.objects.get(id) else {
                self.failed = true;
                return None;
            };
            value = next;
        }
        self.limited = true;
        None
    }

    pub(super) fn dict(&mut self, value: &'a Object) -> Option<&'a Dictionary> {
        self.resolve(value)?.as_dict().ok()
    }

    pub(super) fn array_bounded(
        &mut self,
        value: &'a Object,
        max_len: usize,
    ) -> Option<Vec<&'a Object>> {
        let values = self.resolve(value)?.as_array().ok()?;
        if values.len() > max_len {
            self.limited = true;
            return None;
        }
        #[cfg(test)]
        {
            self.materialized_array_items += values.len();
        }
        Some(values.iter().collect())
    }

    pub(super) fn annotation_array_bounded(
        &mut self,
        value: &'a Object,
        max_len: usize,
    ) -> Option<Vec<&'a Object>> {
        let values = self.array_bounded(value, max_len)?;
        #[cfg(test)]
        {
            self.annotation_materialized_array_items += values.len();
        }
        Some(values)
    }

    pub(super) fn text(&mut self, value: &'a Object) -> Option<String> {
        let value = self.resolve(value)?;
        let bytes = match value {
            Object::String(bytes, _) | Object::Name(bytes) => bytes,
            _ => return None,
        };
        if bytes.len() > MAX_STRING_BYTES || bytes.len() > self.text_remaining {
            self.limited = true;
            return None;
        }
        let text = match value {
            Object::Name(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => decode_pdfjs_text_string(value)?,
        };
        if text.len() > MAX_STRING_BYTES || text.len() > self.text_remaining {
            self.limited = true;
            return None;
        }
        self.text_remaining -= text.len();
        Some(text)
    }
}
