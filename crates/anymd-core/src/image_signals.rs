use std::collections::{BTreeMap, HashMap, HashSet};

use lopdf::{content::Content, Dictionary, Document, Object, ObjectId};
use serde_json::{json, Value};

use self::decode::{bounded_page_content, decode_image, DecodeFailure, DecodedImage};

mod decode;

#[cfg(test)]
mod tests;

const MAX_IMAGES_PER_SOURCE: usize = 128;
const MAX_IMAGE_OPERATIONS_PER_SOURCE: usize = 4_096;
const MAX_XOBJECT_RESOURCES_PER_SOURCE: usize = 16_384;
const MAX_RESOURCE_ANCESTRY_NODES_PER_SOURCE: usize = 4_096;
const MAX_RESOURCE_NAME_BYTES: usize = 1_024;
const MAX_CONTENT_BYTES_PER_PAGE: usize = 4 * 1024 * 1024;
const MAX_PIXELS_PER_IMAGE: usize = 16 * 1024 * 1024;
const MAX_PIXELS_PER_SOURCE: usize = 32 * 1024 * 1024;
const MAX_DECODED_BYTES_PER_IMAGE: usize = 64 * 1024 * 1024;
const MAX_DECODED_BYTES_PER_SOURCE: usize = 128 * 1024 * 1024;
const MAX_ENCODED_BYTES_PER_SOURCE: usize = 64 * 1024 * 1024;

const LIMIT_WARNING: &str =
    "include_images: common raster extraction reached a bounded resource limit; remaining images were omitted.";
const UNSUPPORTED_WARNING: &str =
    "include_images: unsupported or malformed image XObjects were omitted from the bounded common-raster subset.";

#[derive(Default)]
pub(crate) struct ImageSignals {
    pub images: Vec<Value>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct Budget {
    operations: usize,
    pixels: usize,
    decoded_bytes: usize,
    encoded_bytes: usize,
    resource_entries: usize,
    resource_ancestry_nodes: usize,
    limited: bool,
    unsupported: bool,
}

pub(crate) fn extract_image_signals(
    document: &Document,
    pages: &[(u32, ObjectId)],
    selected_pages: &[u32],
) -> ImageSignals {
    let selected = selected_pages.iter().copied().collect::<HashSet<_>>();
    let mut output = ImageSignals::default();
    let mut cache = HashMap::<ObjectId, Option<DecodedImage>>::new();
    let mut budget = Budget::default();

    'pages: for (page_number, page_id) in pages {
        if !selected.contains(page_number) {
            continue;
        }
        let resources = page_xobjects(document, *page_id, &mut budget);
        if budget.limited {
            break;
        }
        let content = match bounded_page_content(document, *page_id) {
            Ok(content) => content,
            Err(DecodeFailure::Limit) => {
                budget.limited = true;
                continue;
            }
            Err(DecodeFailure::Unsupported) => {
                budget.unsupported = true;
                continue;
            }
        };
        let Ok(operations) = Content::decode(&content).map(|content| content.operations) else {
            budget.unsupported = true;
            continue;
        };
        let mut page_index = 0usize;
        for operation in operations {
            if operation.operator != "Do" {
                continue;
            }
            budget.operations = budget.operations.saturating_add(1);
            if budget.operations > MAX_IMAGE_OPERATIONS_PER_SOURCE
                || output.images.len() >= MAX_IMAGES_PER_SOURCE
            {
                budget.limited = true;
                break 'pages;
            }
            let Some(name) = operation
                .operands
                .first()
                .and_then(|value| value.as_name().ok())
            else {
                budget.unsupported = true;
                continue;
            };
            let operation_index = page_index;
            page_index += 1;
            let Some(object_id) = resources.get(name) else {
                // Form XObjects and inline/direct objects are deliberately outside this slice.
                continue;
            };
            let decoded = if let Some(cached) = cache.get(object_id) {
                cached.clone()
            } else {
                let value = decode_image(document, *object_id, &mut budget);
                cache.insert(*object_id, value.clone());
                value
            };
            let Some(decoded) = decoded else {
                continue;
            };
            if !admit_encoded_output(&mut budget, decoded.encoded_bytes) {
                break 'pages;
            }
            // Decoded pixels are charged once per unique object; encoded payload bytes are
            // charged per emitted paint operation because each public image part duplicates it.
            output.images.push(json!({
                "page": page_number,
                "index": operation_index,
                "width": decoded.width,
                "height": decoded.height,
                "format": decoded.format,
                "data": decoded.data,
            }));
        }
    }

    if budget.limited {
        output.warnings.push(LIMIT_WARNING.into());
    }
    if budget.unsupported {
        output.warnings.push(UNSUPPORTED_WARNING.into());
    }
    output
}

fn page_xobjects(
    document: &Document,
    page_id: ObjectId,
    budget: &mut Budget,
) -> BTreeMap<Vec<u8>, ObjectId> {
    let mut output = BTreeMap::new();
    let mut current = page_id;
    let mut seen = HashSet::new();
    loop {
        budget.resource_ancestry_nodes = budget.resource_ancestry_nodes.saturating_add(1);
        if budget.resource_ancestry_nodes > MAX_RESOURCE_ANCESTRY_NODES_PER_SOURCE {
            budget.limited = true;
            return output;
        }
        if !seen.insert(current) {
            budget.unsupported = true;
            return output;
        }
        let Ok(node) = document.get_dictionary(current) else {
            budget.unsupported = true;
            return output;
        };
        if let Ok(resources) = node.get(b"Resources") {
            if let Some(resources) = resolve_dictionary(document, resources) {
                // PDF.js merges inheritable Resources at the top level, without
                // merging sub-dictionaries. Continue until the nearest dictionary
                // that owns XObject; that key then shadows all ancestor XObjects.
                if resources.get(b"XObject").is_ok() {
                    collect_xobjects(document, resources, &mut output, budget);
                    return output;
                }
            } else {
                budget.unsupported = true;
                return output;
            }
        }
        let Some(parent) = node
            .get(b"Parent")
            .ok()
            .and_then(|value| value.as_reference().ok())
        else {
            return output;
        };
        current = parent;
    }
}

fn resolve_dictionary<'a>(document: &'a Document, value: &'a Object) -> Option<&'a Dictionary> {
    resolve(document, value).ok()?.as_dict().ok()
}

fn collect_xobjects(
    document: &Document,
    resources: &Dictionary,
    output: &mut BTreeMap<Vec<u8>, ObjectId>,
    budget: &mut Budget,
) {
    let Some(dictionary) = resources
        .get(b"XObject")
        .ok()
        .and_then(|value| resolve(document, value).ok())
        .and_then(|value| value.as_dict().ok())
    else {
        return;
    };
    for (name, value) in dictionary {
        budget.resource_entries = budget.resource_entries.saturating_add(1);
        if budget.resource_entries > MAX_XOBJECT_RESOURCES_PER_SOURCE {
            budget.limited = true;
            return;
        }
        if name.len() > MAX_RESOURCE_NAME_BYTES {
            budget.unsupported = true;
            continue;
        }
        let Object::Reference(object_id) = value else {
            continue;
        };
        output.entry(name.clone()).or_insert(*object_id);
    }
}

fn reserve_decoded_image(budget: &mut Budget, pixels: usize, decoded_bytes: usize) -> bool {
    let Some(total_pixels) = budget.pixels.checked_add(pixels) else {
        budget.limited = true;
        return false;
    };
    let Some(total_bytes) = budget.decoded_bytes.checked_add(decoded_bytes) else {
        budget.limited = true;
        return false;
    };
    if pixels > MAX_PIXELS_PER_IMAGE
        || decoded_bytes > MAX_DECODED_BYTES_PER_IMAGE
        || total_pixels > MAX_PIXELS_PER_SOURCE
        || total_bytes > MAX_DECODED_BYTES_PER_SOURCE
    {
        budget.limited = true;
        return false;
    }
    budget.pixels = total_pixels;
    budget.decoded_bytes = total_bytes;
    true
}

fn admit_encoded_output(budget: &mut Budget, bytes: usize) -> bool {
    let Some(total) = budget.encoded_bytes.checked_add(bytes) else {
        budget.limited = true;
        return false;
    };
    if total > MAX_ENCODED_BYTES_PER_SOURCE {
        budget.limited = true;
        return false;
    }
    budget.encoded_bytes = total;
    true
}

fn positive_usize(value: &Object) -> Option<usize> {
    let value = value.as_i64().ok()?;
    usize::try_from(value).ok().filter(|value| *value > 0)
}

fn resolve<'a>(document: &'a Document, value: &'a Object) -> lopdf::Result<&'a Object> {
    match value {
        Object::Reference(id) => document.get_object(*id),
        value => Ok(value),
    }
}
