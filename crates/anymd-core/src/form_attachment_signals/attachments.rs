use std::collections::{HashMap, HashSet, VecDeque};

use lopdf::Dictionary;

use super::{
    walker::{Walker, MAX_DEPTH, MAX_ENTRIES},
    Attachment,
};

pub(super) fn extract_attachments<'a>(
    walker: &mut Walker<'a>,
    catalog: &'a Dictionary,
) -> Option<Vec<Attachment>> {
    let names = walker.dict(catalog.get(b"Names").ok()?)?;
    let root = names.get(b"EmbeddedFiles").ok()?;
    let mut queue = VecDeque::from([(root, 0usize)]);
    let mut tree_node_budget = MAX_ENTRIES.saturating_sub(1);
    let mut pair_budget = MAX_ENTRIES;
    let mut visited = HashSet::new();
    let mut pairs = Vec::new();
    while let Some((node, depth)) = queue.pop_front() {
        if depth >= MAX_DEPTH || pairs.len() >= MAX_ENTRIES {
            walker.limited = true;
            return None;
        }
        if let Ok(id) = node.as_reference() {
            if !visited.insert(id) {
                walker.failed = true;
                return None;
            }
        }
        let dict = walker.dict(node)?;
        if let Ok(kids_value) = dict.get(b"Kids") {
            if let Some(kids) = walker.array_bounded(kids_value, tree_node_budget) {
                tree_node_budget -= kids.len();
                for kid in kids {
                    queue.push_back((kid, depth + 1));
                }
            }
            continue;
        }
        let names = dict
            .get(b"Names")
            .ok()
            .and_then(|value| walker.array_bounded(value, pair_budget.saturating_mul(2)));
        if let Some(names) = names {
            // pdf.js NameTree: complete key/value pairs are admitted; a trailing
            // unpaired key materializes as an unnamed attachment instead of
            // failing the whole EmbeddedFiles surface.
            let pair_count = names.len() / 2;
            let orphan_count = usize::from(names.len() % 2 == 1);
            let needed = pair_count + orphan_count;
            if needed > pair_budget {
                walker.limited = true;
                return None;
            }
            pair_budget -= needed;
            for pair in names.as_chunks::<2>().0.iter() {
                pairs.push((pair[0], Some(pair[1])));
            }
            if orphan_count == 1 {
                if let Some(key) = names.last() {
                    pairs.push((*key, None));
                }
            }
        }
    }
    let mut output: Vec<Attachment> = Vec::new();
    let mut positions = HashMap::new();
    for (key, spec) in pairs {
        walker.entries += 1;
        if walker.entries > MAX_ENTRIES {
            walker.limited = true;
            return None;
        }
        let name = walker.text(key)?;
        let attachment = if let Some(spec) = spec {
            let spec = walker.dict(spec)?;
            let filename = filename(walker, spec);
            let description = spec
                .get(b"Desc")
                .ok()
                .and_then(|v| walker.text(v))
                .filter(|v| !v.is_empty());
            let size_bytes = embedded_size(walker, spec);
            Attachment {
                name: name.clone(),
                filename,
                description,
                size_bytes,
            }
        } else {
            Attachment {
                name: name.clone(),
                filename: Some("unnamed".into()),
                description: None,
                size_bytes: None,
            }
        };
        if let Some(index) = positions.get(&name).copied() {
            output[index] = attachment
        } else {
            positions.insert(name, output.len());
            output.push(attachment)
        }
    }
    if walker.failed {
        return None;
    }
    (!output.is_empty()).then_some(output)
}

pub(super) fn filename<'a>(walker: &mut Walker<'a>, spec: &'a Dictionary) -> Option<String> {
    let mut raw = None;
    for key in [b"UF".as_slice(), b"F", b"Unix", b"Mac", b"DOS"] {
        if let Ok(value) = spec.get(key) {
            raw = walker.text(value);
            break;
        }
    }
    let normalized = raw.unwrap_or_default().replace('\\', "/");
    Some(
        normalized
            .rsplit('/')
            .next()
            .filter(|part| !part.is_empty())
            .unwrap_or("unnamed")
            .to_string(),
    )
}

fn embedded_size<'a>(walker: &mut Walker<'a>, spec: &'a Dictionary) -> Option<usize> {
    let ef = walker.dict(spec.get(b"EF").ok()?)?;
    let stream = [b"UF".as_slice(), b"F", b"Unix", b"Mac", b"DOS"]
        .into_iter()
        .find_map(|key| ef.get(key).ok())?;
    let stream = walker.resolve(stream)?;
    let stream = stream.as_stream().ok()?;
    stream
        .dict
        .get(b"Filter")
        .is_err()
        .then_some(stream.content.len())
}
