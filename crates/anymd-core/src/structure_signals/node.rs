use std::collections::{HashMap, HashSet};

use lopdf::ObjectId;

use super::walk::{object_list_preserving_scalar_ref, Walker, PDFJS_MAX_DEPTH};
use super::{StructureChild, StructureContent, StructureNode};

pub(super) struct PageSerialization<'a> {
    pub(super) page_id: ObjectId,
    pub(super) role_map: &'a HashMap<Vec<u8>, Vec<u8>>,
    pub(super) included: &'a HashSet<ObjectId>,
    pub(super) annotation_elements: &'a HashSet<ObjectId>,
}

impl PageSerialization<'_> {
    pub(super) fn serialize_roots(
        &self,
        walker: &mut Walker<'_, '_>,
        roots: Vec<ObjectId>,
    ) -> Option<Vec<StructureChild>> {
        let mut children = Vec::new();
        for id in roots {
            if let Some(node) = self.serialize_node(walker, id, 0) {
                children.push(StructureChild::Node(node));
            }
            if walker.failed || walker.limited {
                return None;
            }
        }
        Some(children)
    }

    fn serialize_node(
        &self,
        walker: &mut Walker<'_, '_>,
        id: ObjectId,
        depth: usize,
    ) -> Option<StructureNode> {
        let page_id = self.page_id;
        if depth > PDFJS_MAX_DEPTH {
            return None;
        }
        walker.admit_node()?;
        let dict = walker.document.objects.get(&id)?.as_dict().ok()?;
        let raw_role = dict
            .get(b"S")
            .ok()
            .and_then(|value| walker.resolve(value))
            .and_then(|value| value.as_name().ok())
            .unwrap_or_default();
        let role = self
            .role_map
            .get(raw_role)
            .map(Vec::as_slice)
            .unwrap_or(raw_role);
        let role = if role.is_empty() {
            walker.admit_text("Unknown".len())?;
            "Unknown".into()
        } else {
            walker.admit_text(lossy_utf8_len(role))?;
            String::from_utf8_lossy(role).trim().to_string()
        };
        let inherited_page = dict
            .get(b"Pg")
            .ok()
            .and_then(|value| value.as_reference().ok());
        let kids = dict
            .get(b"K")
            .ok()
            .map(|value| object_list_preserving_scalar_ref(walker, value))
            .transpose()
            .ok()
            .flatten()
            .unwrap_or_default();
        let mut children = Vec::new();
        for kid in kids {
            walker.admit_node()?;
            if let Ok(child_id) = kid.as_reference() {
                if self.included.contains(&child_id) {
                    if let Some(node) = self.serialize_node(walker, child_id, depth + 1) {
                        children.push(StructureChild::Node(node));
                    }
                    continue;
                }
            }
            let resolved = walker.resolve(kid)?;
            if let Ok(mcid) = resolved.as_i64() {
                if inherited_page == Some(page_id) {
                    let id_len = 1 + format_id_len(page_id) + 3 + i64_decimal_len(mcid);
                    walker.admit_text(id_len)?;
                    children.push(content(
                        walker,
                        "content",
                        Some(format_mcid_id(page_id, mcid)),
                    )?);
                }
                continue;
            }
            let Ok(content_dict) = resolved.as_dict() else {
                continue;
            };
            let content_page = content_dict
                .get(b"Pg")
                .ok()
                .and_then(|value| value.as_reference().ok())
                .or(inherited_page);
            if content_page != Some(page_id) {
                continue;
            }
            let kind = content_dict
                .get(b"Type")
                .ok()
                .and_then(|value| walker.resolve(value))
                .and_then(|value| value.as_name().ok());
            if kind == Some(b"MCR") {
                let mcid = content_dict
                    .get(b"MCID")
                    .ok()
                    .and_then(|value| walker.integer(value));
                let id = if let Some(mcid) = mcid {
                    let len = 1 + format_id_len(page_id) + 3 + i64_decimal_len(mcid);
                    walker.admit_text(len)?;
                    Some(format_mcid_id(page_id, mcid))
                } else {
                    None
                };
                children.push(content(walker, "content", id)?);
            } else if kind == Some(b"OBJR") {
                let object_id = content_dict
                    .get(b"Obj")
                    .ok()
                    .and_then(|value| value.as_reference().ok());
                let annotation = self.annotation_elements.contains(&id)
                    || object_id.is_some_and(|id| walker.annotation_ids.contains(&id));
                let id = if let Some(id) = object_id {
                    let prefix = if annotation { "pdfjs_internal_id_" } else { "" };
                    walker.admit_text(prefix.len() + format_id_len(id))?;
                    Some(if annotation {
                        format_annotation_id(id)
                    } else {
                        format_id(id)
                    })
                } else {
                    None
                };
                children.push(content(
                    walker,
                    if annotation { "annotation" } else { "object" },
                    id,
                )?);
            }
        }
        Some(StructureNode { role, children })
    }
}

fn content(walker: &mut Walker<'_, '_>, kind: &str, id: Option<String>) -> Option<StructureChild> {
    walker.admit_text(kind.len())?;
    Some(StructureChild::Content(StructureContent {
        r#type: kind.into(),
        id,
    }))
}

pub(super) fn lossy_utf8_len(mut bytes: &[u8]) -> usize {
    let mut length = 0usize;
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(value) => return length.saturating_add(value.len()),
            Err(error) => {
                length = length
                    .saturating_add(error.valid_up_to())
                    .saturating_add('\u{FFFD}'.len_utf8());
                let skip = error.valid_up_to() + error.error_len().unwrap_or(1);
                bytes = &bytes[skip.min(bytes.len())..];
            }
        }
    }
    length
}

fn format_id_len((num, generation): ObjectId) -> usize {
    (num.checked_ilog10().unwrap_or(0) as usize + 1)
        + 1
        + if generation == 0 {
            0
        } else {
            generation.checked_ilog10().unwrap_or(0) as usize + 1
        }
}

fn i64_decimal_len(value: i64) -> usize {
    value.unsigned_abs().checked_ilog10().unwrap_or(0) as usize + 1 + usize::from(value < 0)
}

fn format_mcid_id((num, generation): ObjectId, mcid: i64) -> String {
    if generation == 0 {
        format!("p{num}R_mc{mcid}")
    } else {
        format!("p{num}R{generation}_mc{mcid}")
    }
}

fn format_annotation_id((num, generation): ObjectId) -> String {
    if generation == 0 {
        format!("pdfjs_internal_id_{num}R")
    } else {
        format!("pdfjs_internal_id_{num}R{generation}")
    }
}

pub(super) fn format_id((num, generation): ObjectId) -> String {
    if generation == 0 {
        format!("{num}R")
    } else {
        format!("{num}R{generation}")
    }
}
