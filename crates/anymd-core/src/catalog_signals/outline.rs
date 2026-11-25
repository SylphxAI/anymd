use std::collections::{HashSet, VecDeque};

use lopdf::{Dictionary, Object};
use serde::Serialize;

use super::outline_destination::{
    normalized_destination, outline_gotor_url, safe_outline_url, Destination,
};
use super::walker::{Walker, MAX_ENTRIES, MAX_TREE_DEPTH};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OutlineItem {
    title: String,
    bold: bool,
    italic: bool,
    color: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    dest: Destination,
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Vec<OutlineItem>>,
}

pub(super) fn extract_outline(
    walker: &mut Walker<'_>,
    catalog: &Dictionary,
) -> Option<Vec<OutlineItem>> {
    let root = walker.dict(catalog.get(b"Outlines").ok()?)?;
    let first = root.get(b"First").ok()?.clone();
    let first_id = first.as_reference().ok()?;
    let mut processed = HashSet::new();
    processed.insert(first_id);
    let mut arena = vec![OutlineArenaNode::default()];
    let mut queue = VecDeque::from([(first, 0usize, 0usize)]);

    while let Some((value, parent, depth)) = queue.pop_front() {
        if depth >= MAX_TREE_DEPTH || walker.entries >= MAX_ENTRIES {
            walker.truncated = true;
            break;
        }
        let Some(object) = walker.resolve_owned(&value) else {
            break;
        };
        if matches!(object, Object::Null) {
            continue;
        }
        let Ok(dict) = object.as_dict() else {
            walker.failed = true;
            break;
        };
        walker.entries += 1;
        let node = arena.len();
        arena.push(OutlineArenaNode {
            item: normalize_outline_item(walker, dict),
            children: Vec::new(),
        });
        arena[parent].children.push(node);

        for (key, child_parent, child_depth) in [
            (b"First".as_slice(), node, depth + 1),
            (b"Next".as_slice(), parent, depth),
        ] {
            let Ok(next) = dict.get(key) else { continue };
            let Ok(next_id) = next.as_reference() else {
                continue;
            };
            if processed.insert(next_id) {
                queue.push_back((next.clone(), child_parent, child_depth));
            }
        }
    }
    if walker.truncated || walker.failed {
        return None;
    }
    Some(build_outline_children(&arena, 0))
}

#[derive(Default)]
struct OutlineArenaNode {
    item: Option<OutlineItem>,
    children: Vec<usize>,
}

fn build_outline_children(arena: &[OutlineArenaNode], parent: usize) -> Vec<OutlineItem> {
    arena[parent]
        .children
        .iter()
        .filter_map(|child| {
            let mut item = arena[*child].item.as_ref()?.clone();
            let children = build_outline_children(arena, *child);
            if !children.is_empty() {
                item.items = Some(children);
            }
            Some(item)
        })
        .collect()
}

fn normalize_outline_item(walker: &mut Walker<'_>, dict: &Dictionary) -> Option<OutlineItem> {
    let title = walker.text(dict.get(b"Title").ok()?)?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let flags = dict
        .get(b"F")
        .ok()
        .and_then(|v| v.as_i64().ok())
        .unwrap_or(0);
    let color = dict
        .get(b"C")
        .ok()
        .and_then(|v| v.as_array().ok())
        .filter(|v| v.len() == 3)
        .and_then(|v| v.iter().map(finite_number).collect::<Option<Vec<_>>>())
        .map(|values| {
            values
                .into_iter()
                .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
                .collect()
        })
        .unwrap_or_else(|| vec![0, 0, 0]);
    let direct_dest = dict
        .get(b"Dest")
        .ok()
        .and_then(|v| normalized_destination(walker, v));
    let action = dict.get(b"A").ok().and_then(|v| walker.dict(v));
    let action_kind = action
        .as_ref()
        .and_then(|a| a.get(b"S").ok())
        .and_then(|v| v.as_name().ok());
    // pdf.js outline actions:
    // - URI supplies absolute-safe url
    // - GoTo supplies dest (winning over /Dest)
    // - Launch and GoToR supply absolute-safe url built from F + "#" + remote D,
    //   and force dest null (relative file specs are dropped by URL validation)
    // - unsupported / missing actions keep dest null unless /Dest is present
    let (url, dest) = match action_kind {
        Some(b"URI") => {
            let url = action
                .as_ref()
                .and_then(|a| a.get(b"URI").ok())
                .and_then(|v| walker.text(v))
                .and_then(|value| safe_outline_url(walker, &value));
            (url, Destination::Null)
        }
        Some(b"GoTo") => {
            let dest = action
                .as_ref()
                .and_then(|a| a.get(b"D").ok())
                .and_then(|v| normalized_destination(walker, v))
                .or(direct_dest)
                .unwrap_or(Destination::Null);
            (None, dest)
        }
        Some(b"Launch") | Some(b"GoToR") => {
            let url = action.as_ref().and_then(|a| outline_gotor_url(walker, a));
            (url, Destination::Null)
        }
        _ => (None, direct_dest.unwrap_or(Destination::Null)),
    };
    Some(OutlineItem {
        title,
        bold: flags & 2 != 0,
        italic: flags & 1 != 0,
        color,
        url,
        dest,
        items: None,
    })
}

fn finite_number(value: &Object) -> Option<f64> {
    let n = match value {
        Object::Integer(v) => *v as f64,
        Object::Real(v) => f64::from(*v),
        _ => return None,
    };
    n.is_finite().then_some(n)
}
