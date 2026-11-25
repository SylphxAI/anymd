use std::collections::HashMap;

use lopdf::{Document, ObjectId};

use super::maps::{read_number_tree, read_role_map};
use super::page::extract_page_tree;
use super::walk::{object_list_preserving_scalar_ref, RequestBudget, Walker};
use super::StructureTreeExtraction;

pub(super) fn extract(
    document: &Document,
    pages: &[(u32, ObjectId)],
    selected_pages: &[u32],
) -> Result<Option<StructureTreeExtraction>, ()> {
    let mut budget = RequestBudget::default();
    if budget.admit_items(selected_pages.len()).is_none() {
        return Err(());
    }
    let mut selected = selected_pages.to_vec();
    budget.materialized(selected.len());
    selected.sort_unstable();
    selected.dedup();
    let Ok(root_value) = document.trailer.get(b"Root") else {
        return Err(());
    };
    let (struct_root, top_level, role_map, parent_tree) = {
        let mut setup = Walker::new(document, &mut budget);
        let Some(catalog) = setup.dict(root_value) else {
            return Err(());
        };
        let struct_root_value = match catalog.get(b"StructTreeRoot") {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let Some(struct_root) = setup.dict(struct_root_value) else {
            return Err(());
        };
        if struct_root
            .get(b"Type")
            .ok()
            .and_then(|value| value.as_name().ok())
            != Some(b"StructTreeRoot")
        {
            return Err(());
        }
        let role_map = read_role_map(&mut setup, struct_root);
        let Some(top_level) = struct_root
            .get(b"K")
            .ok()
            .map(|value| object_list_preserving_scalar_ref(&mut setup, value))
            .transpose()
            .ok()
            .flatten()
        else {
            return Err(());
        };
        let parent_tree = match struct_root.get(b"ParentTree") {
            Ok(value) => match read_number_tree(&mut setup, value) {
                Some(tree) => Some(tree),
                None => return Err(()),
            },
            Err(_) => None,
        };
        if setup.failed || setup.limited {
            return Err(());
        }
        (struct_root, top_level, role_map, parent_tree)
    };
    if budget.admit_items(pages.len()).is_none() {
        return Err(());
    }
    let page_by_number = pages.iter().copied().collect::<HashMap<_, _>>();
    budget.materialized(pages.len());
    let mut output = Vec::new();
    let mut complete = true;
    for page in selected {
        let Some(page_id) = page_by_number.get(&page).copied() else {
            continue;
        };
        let mut walker = Walker::new(document, &mut budget);
        if let Some(tree) = extract_page_tree(
            &mut walker,
            page,
            page_id,
            struct_root,
            &top_level,
            &role_map,
            parent_tree.as_ref(),
        ) {
            output.push(tree);
        } else {
            complete = false;
        }
    }
    Ok(Some(StructureTreeExtraction {
        trees: output,
        complete,
    }))
}
