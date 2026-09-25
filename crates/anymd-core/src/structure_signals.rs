use std::collections::{HashMap, HashSet, VecDeque};

use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::Serialize;
#[cfg(test)]
use serde_json::{json, Value};

const PDFJS_MAX_DEPTH: usize = 40;
const MAX_REQUEST_WORK: usize = 10_000;
const MAX_ARRAY_ITEMS: usize = 10_000;
const MAX_ROLE_MAP_TEXT_BYTES: usize = 1_048_576;

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

    drop(setup);
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

fn extract_page_tree<'a>(
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
    let mut children = Vec::new();
    for id in roots {
        if let Some(node) = serialize_node(
            walker,
            id,
            page_id,
            role_map,
            &included,
            &annotation_elements,
            0,
        ) {
            children.push(StructureChild::Node(node));
        }
        if walker.failed || walker.limited {
            return None;
        }
    }
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

#[allow(clippy::too_many_arguments)]
fn serialize_node(
    walker: &mut Walker<'_, '_>,
    id: ObjectId,
    page_id: ObjectId,
    role_map: &HashMap<Vec<u8>, Vec<u8>>,
    included: &HashSet<ObjectId>,
    annotation_elements: &HashSet<ObjectId>,
    depth: usize,
) -> Option<StructureNode> {
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
    let role = role_map
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
            if included.contains(&child_id) {
                if let Some(node) = serialize_node(
                    walker,
                    child_id,
                    page_id,
                    role_map,
                    included,
                    annotation_elements,
                    depth + 1,
                ) {
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
            let annotation = annotation_elements.contains(&id)
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

fn content(walker: &mut Walker<'_, '_>, kind: &str, id: Option<String>) -> Option<StructureChild> {
    walker.admit_text(kind.len())?;
    Some(StructureChild::Content(StructureContent {
        r#type: kind.into(),
        id,
    }))
}

fn lossy_utf8_len(mut bytes: &[u8]) -> usize {
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
    u32_decimal_len(num)
        + 1
        + if generation == 0 {
            0
        } else {
            u16_decimal_len(generation)
        }
}

fn u32_decimal_len(value: u32) -> usize {
    value.checked_ilog10().unwrap_or(0) as usize + 1
}

fn u16_decimal_len(value: u16) -> usize {
    value.checked_ilog10().unwrap_or(0) as usize + 1
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

fn read_role_map<'a>(
    walker: &mut Walker<'a, '_>,
    root: &'a Dictionary,
) -> HashMap<Vec<u8>, Vec<u8>> {
    let Some(dict) = root
        .get(b"RoleMap")
        .ok()
        .and_then(|value| walker.dict(value))
    else {
        return HashMap::new();
    };
    if walker.admit_items(dict.len()).is_none() {
        return HashMap::new();
    }
    let mut output = HashMap::new();
    for (key, value) in dict.iter() {
        let Some(value) = walker.resolve(value).and_then(|value| value.as_name().ok()) else {
            continue;
        };
        if walker
            .admit_text(key.len().saturating_add(value.len()))
            .is_none()
        {
            return HashMap::new();
        }
        output.insert(key.clone(), value.to_vec());
        walker.materialized(1);
    }
    output
}

fn read_number_tree<'a>(
    walker: &mut Walker<'a, '_>,
    root: &'a Object,
) -> Option<HashMap<i64, &'a Object>> {
    walker.admit_work()?;
    let mut queue = VecDeque::from([(root, 0usize)]);
    walker.materialized(1);
    let mut seen = HashSet::new();
    let mut output = HashMap::new();
    while let Some((value, depth)) = queue.pop_front() {
        if depth > PDFJS_MAX_DEPTH || output.len() > MAX_ARRAY_ITEMS {
            walker.limited = true;
            return None;
        }
        if let Ok(id) = value.as_reference() {
            if !seen.insert(id) {
                walker.failed = true;
                return None;
            }
        }
        if depth > 0 {
            walker.admit_work()?;
        }
        let dict = walker.dict(value)?;
        if let Ok(kids) = dict.get(b"Kids") {
            for kid in object_list(walker, kids).ok()? {
                queue.push_back((kid, depth + 1));
            }
            continue;
        }
        if let Ok(nums) = dict.get(b"Nums") {
            let nums = object_list(walker, nums).ok()?;
            if nums.len() % 2 != 0 || output.len() + nums.len() / 2 > MAX_ARRAY_ITEMS {
                walker.limited = true;
                return None;
            }
            for pair in nums.chunks_exact(2) {
                if let Some(key) = walker.integer(pair[0]) {
                    output.insert(key, pair[1]);
                }
            }
        }
    }
    Some(output)
}

fn object_list<'a>(walker: &mut Walker<'a, '_>, value: &'a Object) -> Result<Vec<&'a Object>, ()> {
    object_list_with_identity(walker, value, false)
}

fn object_list_preserving_scalar_ref<'a>(
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
struct RequestBudget {
    work: usize,
    admitted: usize,
    text_bytes: usize,
    materialized: usize,
}

impl RequestBudget {
    fn admit_items(&mut self, count: usize) -> Option<()> {
        let next = self.admitted.checked_add(count)?;
        if next > MAX_REQUEST_WORK {
            return None;
        }
        self.admitted = next;
        Some(())
    }

    fn admit_work(&mut self) -> Option<()> {
        let next = self.work.checked_add(1)?;
        if next > MAX_REQUEST_WORK {
            return None;
        }
        self.work = next;
        Some(())
    }

    fn admit_text(&mut self, count: usize) -> Option<()> {
        let next = self.text_bytes.checked_add(count)?;
        if next > MAX_ROLE_MAP_TEXT_BYTES {
            return None;
        }
        self.text_bytes = next;
        Some(())
    }

    fn materialized(&mut self, count: usize) {
        self.materialized = self.materialized.saturating_add(count);
    }
}

struct Walker<'a, 'budget> {
    document: &'a Document,
    budget: &'budget mut RequestBudget,
    failed: bool,
    limited: bool,
    annotation_ids: HashSet<ObjectId>,
}

impl<'a, 'budget> Walker<'a, 'budget> {
    fn new(document: &'a Document, budget: &'budget mut RequestBudget) -> Self {
        Self {
            document,
            budget,
            failed: false,
            limited: false,
            annotation_ids: HashSet::new(),
        }
    }

    fn resolve(&mut self, mut value: &'a Object) -> Option<&'a Object> {
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

    fn dict(&mut self, value: &'a Object) -> Option<&'a Dictionary> {
        self.resolve(value)?.as_dict().ok()
    }

    fn integer(&mut self, value: &'a Object) -> Option<i64> {
        self.resolve(value)?.as_i64().ok()
    }

    fn admit_node(&mut self) -> Option<()> {
        if self.budget.admit_items(1).is_none() {
            self.limited = true;
            None
        } else {
            Some(())
        }
    }

    fn admit_items(&mut self, count: usize) -> Option<()> {
        let result = self.budget.admit_items(count);
        if result.is_none() {
            self.limited = true;
        }
        result
    }

    fn admit_text(&mut self, count: usize) -> Option<()> {
        let result = self.budget.admit_text(count);
        if result.is_none() {
            self.limited = true;
        }
        result
    }

    fn admit_work(&mut self) -> Option<()> {
        let result = self.budget.admit_work();
        if result.is_none() {
            self.limited = true;
        }
        result
    }

    fn materialized(&mut self, count: usize) {
        self.budget.materialized(count);
    }
}

fn format_id((num, generation): ObjectId) -> String {
    if generation == 0 {
        format!("{num}R")
    } else {
        format!("{num}R{generation}")
    }
}

#[cfg(test)]
fn normalize_public_tree(raw: &Value) -> Option<Value> {
    fn node(raw: &Value, depth: usize, remaining: &mut usize) -> Option<Value> {
        if depth > PDFJS_MAX_DEPTH || *remaining == 0 {
            return None;
        }
        *remaining -= 1;
        let object = raw.as_object()?;
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Unknown");
        let mut children = Vec::new();
        for child in object
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(object) = child.as_object() else {
                continue;
            };
            if object.contains_key("role") || object.contains_key("children") {
                if let Some(child) = node(child, depth + 1, remaining) {
                    children.push(child);
                }
                continue;
            }
            let kind = object
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if kind.is_empty() && id.is_empty() {
                continue;
            }
            let mut value = json!({"type":if kind.is_empty(){"content"}else{kind}});
            if !id.is_empty() {
                value["id"] = json!(id);
            }
            children.push(value);
        }
        let mut value = json!({"role":role});
        if !children.is_empty() {
            value["children"] = json!(children);
        }
        Some(value)
    }
    let mut remaining = MAX_REQUEST_WORK;
    node(raw, 0, &mut remaining)
}

#[cfg(test)]
mod tests;
