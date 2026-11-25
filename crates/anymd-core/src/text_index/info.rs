//! Project PDF document info, catalog metadata, and form flags.

use std::collections::BTreeMap;

use pdf_extract::{Document, Object};
use serde_json::{json, Value};

use super::PdfInfo;
use crate::pdfjs_text::decode_pdfjs_text_string;

fn xfa_present(doc: &Document, value: &Object) -> bool {
    match value {
        Object::Array(items) => !items.is_empty(),
        Object::Reference(id) => doc
            .get_object(*id)
            .ok()
            .is_some_and(|obj| xfa_present(doc, obj)),
        Object::Stream(stream) => !stream.content.is_empty(),
        _ => false,
    }
}

fn has_only_document_signatures(doc: &Document, fields: &[Object], depth: usize) -> bool {
    const RECURSION_LIMIT: usize = 10;
    if depth > RECURSION_LIMIT || fields.is_empty() {
        return false;
    }
    fields.iter().all(|field| {
        let resolved = match field {
            Object::Reference(id) => doc.get_object(*id).ok(),
            other => Some(other),
        };
        let Some(Object::Dictionary(dict)) = resolved else {
            return false;
        };
        if let Some(kids) = dict.get(b"Kids").ok().and_then(|value| match value {
            Object::Reference(id) => doc.get_object(*id).ok().and_then(|obj| match obj {
                Object::Array(arr) => Some(arr.clone()),
                _ => None,
            }),
            Object::Array(arr) => Some(arr.clone()),
            _ => None,
        }) {
            return has_only_document_signatures(doc, &kids, depth + 1);
        }
        let is_signature = dict
            .get(b"FT")
            .ok()
            .and_then(|value| value.as_name().ok())
            .is_some_and(|name| name == b"Sig");
        let is_invisible = dict
            .get(b"Rect")
            .ok()
            .and_then(|value| match value {
                Object::Reference(id) => doc.get_object(*id).ok().and_then(|obj| match obj {
                    Object::Array(arr) => Some(arr.clone()),
                    _ => None,
                }),
                Object::Array(arr) => Some(arr.clone()),
                _ => None,
            })
            .is_some_and(|rect| {
                !rect.is_empty()
                    && rect.iter().all(|entry| match entry {
                        Object::Integer(0) => true,
                        Object::Real(v) => *v == 0.0,
                        Object::Reference(id) => doc.get_object(*id).ok().is_some_and(|obj| {
                            matches!(obj, Object::Integer(0))
                                || matches!(obj, Object::Real(v) if *v == 0.0)
                        }),
                        _ => false,
                    })
            });
        is_signature && is_invisible
    })
}

pub(crate) fn read_pdf_info(doc: &Document) -> PdfInfo {
    let mut fields = BTreeMap::new();
    let info_object = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|object| match object {
            Object::Reference(id) => doc.get_object(*id).ok(),
            Object::Dictionary(_) => Some(object),
            _ => None,
        });
    if let Some(info) = info_object.and_then(|object| object.as_dict().ok()) {
        const STANDARD_STRING_KEYS: &[&[u8]] = &[
            b"Title",
            b"Author",
            b"Subject",
            b"Keywords",
            b"Creator",
            b"Producer",
            b"CreationDate",
            b"ModDate",
        ];
        let mut custom = serde_json::Map::new();
        for (key_bytes, value) in info.iter() {
            let Ok(key) = std::str::from_utf8(key_bytes) else {
                continue;
            };
            if STANDARD_STRING_KEYS
                .iter()
                .any(|candidate| *candidate == key_bytes)
            {
                if let Some(decoded) = decode_pdfjs_text_string(value) {
                    fields.insert(key.to_string(), Value::String(decoded));
                }
                continue;
            }
            if key_bytes == b"Trapped" {
                // pdf.js: only Name values are admitted for Trapped.
                if let Ok(name) = value.as_name() {
                    if let Ok(name) = std::str::from_utf8(name) {
                        fields.insert("Trapped".into(), json!({ "name": name }));
                    }
                }
                continue;
            }
            // pdf.js default branch: string/number/boolean/Name become Custom[key].
            let custom_value = match value {
                Object::String(_, _) => decode_pdfjs_text_string(value).map(Value::String),
                Object::Integer(n) => Some(json!(*n)),
                Object::Real(n) if n.is_finite() => Some(json!(f64::from(*n))),
                Object::Boolean(b) => Some(json!(*b)),
                Object::Name(name) => std::str::from_utf8(name)
                    .ok()
                    .map(|name| json!({ "name": name })),
                _ => None,
            };
            if let Some(custom_value) = custom_value {
                custom.insert(key.to_string(), custom_value);
            }
        }
        if !custom.is_empty() {
            fields.insert("Custom".into(), Value::Object(custom));
        }
    }
    let catalog = doc.catalog().ok();
    let language = catalog.and_then(|catalog| {
        catalog
            .get(b"Lang")
            .ok()
            .and_then(decode_pdfjs_text_string)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    });
    let encrypt_filter_name = doc
        .trailer
        .get(b"Encrypt")
        .ok()
        .and_then(|value| match value {
            Object::Reference(id) => doc.get_object(*id).ok(),
            Object::Dictionary(_) => Some(value),
            _ => None,
        })
        .and_then(|value| value.as_dict().ok())
        .and_then(|dict| dict.get(b"Filter").ok())
        .and_then(|value| value.as_name().ok())
        .and_then(|value| std::str::from_utf8(value).ok())
        .map(|value| value.to_string())
        .filter(|value| !value.is_empty());
    // pdf.js IsLinearized requires first-object Linearization.create validation
    // against the exact source stream length. Document object graphs alone are
    // insufficient and over-admit; callers with source bytes should override.
    let is_linearized = false;
    let acroform = catalog.and_then(|catalog| {
        catalog.get(b"AcroForm").ok().and_then(|value| match value {
            Object::Reference(id) => doc.get_object(*id).ok(),
            Object::Dictionary(_) => Some(value),
            _ => None,
        })
    });
    let acroform_dict = acroform.and_then(|value| value.as_dict().ok());
    // Match pdf.js formInfo:
    // hasFields = Fields is a non-empty array
    // hasXfa = XFA array non-empty OR non-empty stream
    // hasSignatures = SigFlags & 0x1
    // hasOnlyDocumentSignatures = every leaf field is invisible Sig
    // hasAcroForm = hasFields && !hasOnlyDocumentSignatures
    let fields_obj = acroform_dict.and_then(|dict| dict.get(b"Fields").ok());
    let fields_array = fields_obj.and_then(|value| match value {
        Object::Reference(id) => doc.get_object(*id).ok().and_then(|obj| obj.as_array().ok()),
        Object::Array(arr) => Some(arr),
        _ => None,
    });
    let has_fields = fields_array.is_some_and(|arr| !arr.is_empty());
    let is_xfa_present = acroform_dict
        .and_then(|dict| dict.get(b"XFA").ok())
        .is_some_and(|value| xfa_present(doc, value));
    let is_collection_present = catalog
        .and_then(|catalog| catalog.get(b"Collection").ok())
        .is_some();
    let is_signatures_present = acroform_dict
        .and_then(|dict| dict.get(b"SigFlags").ok())
        .and_then(|value| value.as_i64().ok())
        .is_some_and(|flags| flags & 1 != 0);
    let has_only_document_signatures = is_signatures_present
        && fields_array.is_some_and(|arr| has_only_document_signatures(doc, arr, 0));
    let is_acroform_present = has_fields && !has_only_document_signatures;

    PdfInfo {
        format_version: doc.version.clone(),
        fields,
        language,
        encrypt_filter_name,
        is_linearized,
        is_acroform_present,
        is_xfa_present,
        is_collection_present,
        is_signatures_present,
    }
}
