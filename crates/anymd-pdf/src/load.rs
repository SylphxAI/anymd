use std::path::Path;

use pdf_extract::{Document, LoadOptions, Object, ObjectId};

use crate::LayoutError;

/// Objects that text layout never reads: link and widget annotations and the
/// tagged-PDF structure tree. A tagged book carries tens of thousands of them
/// (about 130 MB of the 142 MB heap for the 492-page NIST SP 800-53r5), so
/// they are dropped while the file is parsed instead of held until the end.
/// Layout reads page content, fonts, images, the outline and the Info title
/// only; none of those reach these objects.
///
/// lopdf keeps the object in place when it is read directly and takes the
/// returned one when it comes out of an object stream, so a kept object is
/// returned as a copy.
fn keep_for_layout(id: ObjectId, object: &mut Object) -> Option<(ObjectId, Object)> {
    if let Object::Dictionary(dict) = object {
        let name = |key: &[u8]| dict.get(key).ok().and_then(|value| value.as_name().ok());
        let droppable = match name(b"Type") {
            Some(b"Annot") | Some(b"StructElem") | Some(b"MCR") | Some(b"OBJR") => true,
            Some(_) => false,
            // Structure elements and annotations may omit /Type.
            None => {
                (dict.has(b"S") && dict.has(b"P") && (dict.has(b"K") || dict.has(b"Pg")))
                    || (dict.has(b"Subtype") && dict.has(b"Rect"))
            }
        };
        if droppable {
            return None;
        }
    }
    Some((id, object.clone()))
}

fn layout_options() -> LoadOptions {
    LoadOptions::with_filter(keep_for_layout)
}

/// Drop every object that cannot be reached from the trailer. Once the
/// annotations and the structure tree are gone, what only they pointed at
/// (link actions and destinations, the structure parent tree) is dead weight
/// too, and the page tree, fonts, images and outline are all reached from the
/// trailer, so layout never looks for anything else. A trailer without a
/// reachable `/Root` leaves the document as it is.
fn prune_unreachable(doc: &mut Document) {
    fn visit(object: &Object, stack: &mut Vec<ObjectId>) {
        match object {
            Object::Reference(id) => stack.push(*id),
            Object::Array(items) => items.iter().for_each(|item| visit(item, stack)),
            Object::Dictionary(dict) => dict.iter().for_each(|(_, value)| visit(value, stack)),
            Object::Stream(stream) => stream
                .dict
                .iter()
                .for_each(|(_, value)| visit(value, stack)),
            _ => {}
        }
    }
    if !matches!(doc.trailer.get(b"Root"), Ok(Object::Reference(_))) {
        return;
    }
    let mut stack = Vec::new();
    doc.trailer
        .iter()
        .for_each(|(_, value)| visit(value, &mut stack));
    let mut reached = std::collections::HashSet::new();
    while let Some(id) = stack.pop() {
        if !reached.insert(id) {
            continue;
        }
        if let Some(object) = doc.objects.get(&id) {
            visit(object, &mut stack);
        }
    }
    doc.objects.retain(|id, _| reached.contains(id));
    give_back_freed_memory();
}

/// Parsing a document, and extracting a chunk of pages on worker threads,
/// peak well above what is kept afterwards; glibc holds the freed pages in
/// per-thread arenas for reuse, so hand them back and the process stays at
/// the size of what it actually holds.
pub(crate) fn give_back_freed_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` takes no pointers and only releases free heap pages.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Open a PDF from disk (decrypting with the empty password when needed).
pub fn load_document(path: &Path) -> Result<Document, LayoutError> {
    let mut doc = Document::load_with_options(path, layout_options())
        .map_err(|err| LayoutError::new(format!("Failed to open PDF: {err}")))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            LayoutError::new(format!("PDF is encrypted and needs a password: {err}"))
        })?;
    }
    prune_unreachable(&mut doc);
    Ok(doc)
}

/// Open a PDF from memory (decrypting with the empty password when needed).
pub fn load_document_bytes(bytes: &[u8]) -> Result<Document, LayoutError> {
    let mut doc = Document::load_mem_with_options(bytes, layout_options())
        .map_err(|err| LayoutError::new(format!("Failed to open PDF: {err}")))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            LayoutError::new(format!("PDF is encrypted and needs a password: {err}"))
        })?;
    }
    prune_unreachable(&mut doc);
    Ok(doc)
}

/// Page count without extracting text.
pub fn page_count(doc: &Document) -> u32 {
    u32::try_from(doc.get_pages().len()).unwrap_or(u32::MAX)
}
