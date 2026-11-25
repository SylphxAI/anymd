use std::collections::HashSet;

use lopdf::{Dictionary, Document, Object, ObjectId};

pub(super) const PDFJS_MAX_DEPTH: usize = 40;
pub(super) const MAX_REQUEST_WORK: usize = 10_000;
pub(super) const MAX_ARRAY_ITEMS: usize = 10_000;
pub(super) const MAX_ROLE_MAP_TEXT_BYTES: usize = 1_048_576;

pub(super) fn object_list<'a>(
    walker: &mut Walker<'a, '_>,
    value: &'a Object,
) -> Result<Vec<&'a Object>, ()> {
    object_list_with_identity(walker, value, false)
}

pub(super) fn object_list_preserving_scalar_ref<'a>(
    walker: &mut Walker<'a, '_>,
    value: &'a Object,
) -> Result<Vec<&'a Object>, ()> {
    object_list_with_identity(walker, value, true)
}

fn object_list_with_identity<'a>(
    walker: &mut Walker<'a, '_>,
    value: &'a Object,
    preserve_scalar_ref: bool,
) -> Result<Vec<&'a Object>, ()> {
    let resolved = walker.resolve(value).ok_or(())?;
    if let Ok(values) = resolved.as_array() {
        if values.len() > MAX_ARRAY_ITEMS || walker.admit_items(values.len()).is_none() {
            walker.limited = true;
            return Err(());
        }
        let output = values.iter().collect::<Vec<_>>();
        walker.materialized(output.len());
        Ok(output)
    } else if matches!(resolved, Object::Null) {
        Ok(Vec::new())
    } else {
        walker.admit_items(1).ok_or(())?;
        // PDF.js structure /K traversal retains scalar Ref identity even though
        // it fetches the target. Other array-like surfaces (notably a scalar
        // ParentTree value) expose the fetched value instead. Keep that
        // distinction explicit: a generic resolver silently changes semantics.
        let output = vec![
            if preserve_scalar_ref && matches!(value, Object::Reference(_)) {
                value
            } else {
                resolved
            },
        ];
        walker.materialized(1);
        Ok(output)
    }
}

#[derive(Default)]
pub(super) struct RequestBudget {
    pub(super) work: usize,
    pub(super) admitted: usize,
    pub(super) text_bytes: usize,
    pub(super) materialized: usize,
}

impl RequestBudget {
    pub(super) fn admit_items(&mut self, count: usize) -> Option<()> {
        let next = self.admitted.checked_add(count)?;
        if next > MAX_REQUEST_WORK {
            return None;
        }
        self.admitted = next;
        Some(())
    }

    pub(super) fn admit_work(&mut self) -> Option<()> {
        let next = self.work.checked_add(1)?;
        if next > MAX_REQUEST_WORK {
            return None;
        }
        self.work = next;
        Some(())
    }

    pub(super) fn admit_text(&mut self, count: usize) -> Option<()> {
        let next = self.text_bytes.checked_add(count)?;
        if next > MAX_ROLE_MAP_TEXT_BYTES {
            return None;
        }
        self.text_bytes = next;
        Some(())
    }

    pub(super) fn materialized(&mut self, count: usize) {
        self.materialized = self.materialized.saturating_add(count);
    }
}

pub(super) struct Walker<'a, 'budget> {
    pub(super) document: &'a Document,
    budget: &'budget mut RequestBudget,
    pub(super) failed: bool,
    pub(super) limited: bool,
    pub(super) annotation_ids: HashSet<ObjectId>,
}

impl<'a, 'budget> Walker<'a, 'budget> {
    pub(super) fn new(document: &'a Document, budget: &'budget mut RequestBudget) -> Self {
        Self {
            document,
            budget,
            failed: false,
            limited: false,
            annotation_ids: HashSet::new(),
        }
    }

    pub(super) fn resolve(&mut self, mut value: &'a Object) -> Option<&'a Object> {
        let mut seen = HashSet::new();
        for _ in 0..=PDFJS_MAX_DEPTH {
            let Object::Reference(id) = value else {
                return Some(value);
            };
            if self.budget.admit_work().is_none() {
                self.limited = true;
                return None;
            }
            if !seen.insert(*id) {
                self.failed = true;
                return None;
            }
            value = self.document.objects.get(id)?;
        }
        self.limited = true;
        None
    }

    pub(super) fn dict(&mut self, value: &'a Object) -> Option<&'a Dictionary> {
        self.resolve(value)?.as_dict().ok()
    }

    pub(super) fn integer(&mut self, value: &'a Object) -> Option<i64> {
        self.resolve(value)?.as_i64().ok()
    }

    pub(super) fn admit_node(&mut self) -> Option<()> {
        if self.budget.admit_items(1).is_none() {
            self.limited = true;
            None
        } else {
            Some(())
        }
    }

    pub(super) fn admit_items(&mut self, count: usize) -> Option<()> {
        let result = self.budget.admit_items(count);
        if result.is_none() {
            self.limited = true;
        }
        result
    }

    pub(super) fn admit_text(&mut self, count: usize) -> Option<()> {
        let result = self.budget.admit_text(count);
        if result.is_none() {
            self.limited = true;
        }
        result
    }

    pub(super) fn admit_work(&mut self) -> Option<()> {
        let result = self.budget.admit_work();
        if result.is_none() {
            self.limited = true;
        }
        result
    }

    pub(super) fn materialized(&mut self, count: usize) {
        self.budget.materialized(count);
    }
}
