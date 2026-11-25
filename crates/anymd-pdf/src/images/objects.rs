//! Image XObject lookup and PDF dictionary access.

use pdf_extract::{Dictionary, Document, Object, ObjectId, Stream};

pub(super) fn deref<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    doc.dereference(object).ok().map(|(_, object)| object)
}

pub(super) fn entry<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    dict.get(key).ok().and_then(|object| deref(doc, object))
}

pub(super) fn int(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<i64> {
    entry(doc, dict, key)?.as_i64().ok()
}

pub(crate) fn image_stream(doc: &Document, id: ObjectId) -> Option<&Stream> {
    let stream = doc.get_object(id).ok()?.as_stream().ok()?;
    let subtype = stream.dict.get(b"Subtype").ok()?.as_name().ok()?;
    (subtype == &b"Image"[..]).then_some(stream)
}

pub(crate) fn dims(doc: &Document, stream: &Stream) -> Option<(u32, u32)> {
    let width = u32::try_from(int(doc, &stream.dict, b"Width")?).ok()?;
    let height = u32::try_from(int(doc, &stream.dict, b"Height")?).ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

pub(super) fn decode_params<'a>(doc: &'a Document, dict: &'a Dictionary) -> Option<&'a Dictionary> {
    let params = entry(doc, dict, b"DecodeParms").or_else(|| entry(doc, dict, b"DP"))?;
    match params {
        Object::Dictionary(dict) => Some(dict),
        Object::Array(items) => deref(doc, items.first()?)?.as_dict().ok(),
        _ => None,
    }
}
