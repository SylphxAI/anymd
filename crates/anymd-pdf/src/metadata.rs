//! Document title and outline.

use std::collections::{BTreeMap, HashSet};

use pdf_extract::{Document, Object};

pub(crate) fn decode_pdf_string(object: &Object) -> Option<String> {
    let bytes = match object {
        Object::String(bytes, _) => bytes,
        _ => return None,
    };
    let text = if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        bytes.iter().map(|&b| b as char).collect()
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

pub(crate) fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    match object {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

pub fn info_title(doc: &Document) -> Option<String> {
    let info = resolve(doc, doc.trailer.get(b"Info").ok()?)?
        .as_dict()
        .ok()?;
    let title = decode_pdf_string(resolve(doc, info.get(b"Title").ok()?)?)?;
    let lower = title.to_ascii_lowercase();
    let junk = lower.starts_with("untitled")
        || lower.starts_with("microsoft word")
        || [
            ".doc", ".docx", ".dvi", ".tex", ".pdf", ".indd", ".qxd", ".ps",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext));
    (!junk && title.chars().count() <= 300).then_some(title)
}

/// Bookmarks with resolved page numbers (bounded walk, cycle-safe).
pub fn outline(doc: &Document) -> Vec<(usize, String, Option<u32>)> {
    let page_of: BTreeMap<(u32, u16), u32> = doc
        .get_pages()
        .into_iter()
        .map(|(number, id)| (id, number))
        .collect();
    let Some(root) = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"Outlines").ok())
        .and_then(|object| resolve(doc, object))
        .and_then(|object| object.as_dict().ok())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    let mut stack: Vec<(usize, Option<&Object>)> = vec![(0, root.get(b"First").ok())];
    while let Some((depth, next)) = stack.pop() {
        let Some(Object::Reference(id)) = next else {
            continue;
        };
        if out.len() >= 2000 || depth > 16 || !visited.insert(*id) {
            continue;
        }
        let Ok(item) = doc.get_object(*id).and_then(Object::as_dict) else {
            continue;
        };
        stack.push((depth, item.get(b"Next").ok()));
        stack.push((depth + 1, item.get(b"First").ok()));
        let Some(title) = item
            .get(b"Title")
            .ok()
            .and_then(|object| resolve(doc, object))
            .and_then(decode_pdf_string)
        else {
            continue;
        };
        let dest = item
            .get(b"Dest")
            .ok()
            .or_else(|| {
                item.get(b"A")
                    .ok()
                    .and_then(|action| resolve(doc, action))
                    .and_then(|action| action.as_dict().ok())
                    .and_then(|action| action.get(b"D").ok())
            })
            .and_then(|dest| resolve(doc, dest));
        let page = match dest {
            Some(Object::Array(parts)) => match parts.first() {
                Some(Object::Reference(page_id)) => page_of.get(page_id).copied(),
                _ => None,
            },
            _ => None,
        };
        out.push((depth, title, page));
    }
    out
}
