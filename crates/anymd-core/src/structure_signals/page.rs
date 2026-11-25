use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Object, ObjectId};

use super::node::PageSerialization;
use super::walk::{object_list, object_list_preserving_scalar_ref, Walker, PDFJS_MAX_DEPTH};
use super::{PageStructureTree, StructureNode};

pub(super) fn extract_page_tree<'a>(
    walker: &mut Walker<'a, '_>,
    page: u32,
    page_id: ObjectId,
    struct_root: &'a Dictionary,
    top_level: &[&'a Object],
    role_map: &HashMap<Vec<u8>, Vec<u8>>,
    parent_tree: Option<&HashMap<i64, &'a Object>>,
) -> Option<PageStructureTree> {
    let page_value = walker.document.objects.get(&page_id)?;
    let page_dict = walker.dict(page_value)?;
    let mut mapped = Vec::new();
    let mut annotation_elements = HashSet::new();
    if let (Some(tree), Some(key)) = (
        parent_tree,
        page_dict
            .get(b"StructParents")
            .ok()
            .and_then(|value| walker.integer(value)),
    ) {
        if let Some(value) = tree.get(&key) {
            for entry in object_list(walker, value).ok()? {
                if let Ok(id) = entry.as_reference() {
                    mapped.push(id);
                }
            }
        }
    }
    if let (Some(tree), Ok(annots_value)) = (parent_tree, page_dict.get(b"Annots")) {
        for annot in object_list(walker, annots_value).ok()? {
            let Ok(annot_id) = annot.as_reference() else {
                continue;
            };
            let Some(annot_dict) = walker.dict(annot) else {
                continue;
            };
            let Some(key) = annot_dict
                .get(b"StructParent")
                .ok()
                .and_then(|value| walker.integer(value))
            else {
                continue;
            };
            let Some(element_id) = tree.get(&key).and_then(|value| value.as_reference().ok())
            else {
                continue;
            };
            mapped.push(element_id);
            annotation_elements.insert(element_id);
            walker.annotation_ids.insert(annot_id);
        }
    }
    if walker.failed || walker.limited {
        return None;
    }

    let top_positions = top_level
        .iter()
        .filter_map(|value| value.as_reference().ok())
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect::<HashMap<_, _>>();
    let direct_only_top_level = top_positions.is_empty()
        && !top_level.is_empty()
        && top_level.iter().all(|value| value.as_dict().is_ok());
    let mut included = HashSet::new();
    if !direct_only_top_level {
        for id in mapped {
            admit_ancestry(walker, id, struct_root, &top_positions, &mut included)?;
        }
    }
    let mut roots = included
        .iter()
        .copied()
        .filter(|id| top_positions.contains_key(id))
        .collect::<Vec<_>>();
    roots.sort_by_key(|id| top_positions.get(id).copied().unwrap_or(usize::MAX));
    let children = PageSerialization {
        page_id,
        role_map,
        included: &included,
        annotation_elements: &annotation_elements,
    }
    .serialize_roots(walker, roots)?;
    walker.admit_text("Root".len())?;
    Some(PageStructureTree {
        page,
        tree: StructureNode {
            role: "Root".into(),
            children,
        },
    })
}

fn admit_ancestry(
    walker: &mut Walker<'_, '_>,
    mut id: ObjectId,
    struct_root: &Dictionary,
    top_positions: &HashMap<ObjectId, usize>,
    included: &mut HashSet<ObjectId>,
) -> Option<()> {
    let mut path = Vec::new();
    let mut seen = HashSet::new();
    for _ in 0..=PDFJS_MAX_DEPTH {
        walker.admit_node()?;
        if included.contains(&id) {
            return Some(());
        }
        if !seen.insert(id) {
            walker.failed = true;
            return None;
        }
        path.push(id);
        let dict = walker.document.objects.get(&id)?.as_dict().ok()?;
        let raw_parent = dict.get(b"P").ok()?;
        let parent = walker.resolve(raw_parent)?;
        let parent_dict = parent.as_dict().ok()?;
        if std::ptr::eq(parent_dict, struct_root) {
            if !top_positions.contains_key(&id) {
                walker.failed = true;
                return None;
            }
            included.extend(path);
            return Some(());
        }
        let parent_id = raw_parent.as_reference().ok()?;
        if !parent_contains(walker, parent_dict, id) {
            walker.failed = true;
            return None;
        }
        id = parent_id;
    }
    walker.limited = true;
    None
}

fn parent_contains<'a>(
    walker: &mut Walker<'a, '_>,
    parent: &'a Dictionary,
    child: ObjectId,
) -> bool {
    parent
        .get(b"K")
        .ok()
        .and_then(|value| object_list_preserving_scalar_ref(walker, value).ok())
        .is_some_and(|kids| {
            kids.into_iter()
                .any(|kid| kid.as_reference().ok() == Some(child))
        })
}
