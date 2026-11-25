use lopdf::{Document, ObjectId};
use serde::Serialize;

mod maps;
mod node;
mod page;
mod request;
mod walk;

#[cfg(test)]
mod normalize;
#[cfg(test)]
mod tests;

#[cfg(test)]
use {
    lopdf::{Dictionary, Object},
    maps::{read_number_tree, read_role_map},
    node::{format_id, lossy_utf8_len},
    normalize::normalize_public_tree,
    serde_json::Value,
    walk::{
        object_list, RequestBudget, Walker, MAX_ARRAY_ITEMS, MAX_REQUEST_WORK,
        MAX_ROLE_MAP_TEXT_BYTES, PDFJS_MAX_DEPTH,
    },
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PageStructureTree {
    page: u32,
    tree: StructureNode,
}

pub(crate) struct StructureTreeExtraction {
    pub(crate) trees: Vec<PageStructureTree>,
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, Serialize)]
struct StructureNode {
    role: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<StructureChild>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum StructureChild {
    Node(StructureNode),
    Content(StructureContent),
}

#[derive(Debug, Clone, Serialize)]
struct StructureContent {
    r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[cfg(test)]
pub(crate) fn extract_structure_trees(
    document: &Document,
    pages: &[(u32, ObjectId)],
    selected_pages: &[u32],
) -> Vec<PageStructureTree> {
    extract_structure_trees_checked(document, pages, selected_pages)
        .ok()
        .flatten()
        .map(|value| value.trees)
        .unwrap_or_default()
}

pub(crate) fn extract_structure_trees_checked(
    document: &Document,
    pages: &[(u32, ObjectId)],
    selected_pages: &[u32],
) -> Result<Option<StructureTreeExtraction>, ()> {
    request::extract(document, pages, selected_pages)
}
