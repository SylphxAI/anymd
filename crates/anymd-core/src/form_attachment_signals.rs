use serde::Serialize;
use serde_json::Value;

mod attachments;
mod extract;
mod forms;
mod walker;

pub(crate) use extract::extract_form_attachment_signals;

#[derive(Default)]
pub(crate) struct FormAttachmentSignals {
    pub form_fields: Option<Vec<FormField>>,
    pub attachments: Option<Vec<Attachment>>,
    pub warnings: Vec<String>,
    #[cfg(test)]
    form_materialized_array_items: usize,
    #[cfg(test)]
    form_annotation_materialized_array_items: usize,
    #[cfg(test)]
    attachment_materialized_array_items: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct FormField {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    editable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bounding_box: Option<BoxValue>,
}

#[derive(Debug, Serialize)]
struct BoxValue {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct Attachment {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<usize>,
}

#[cfg(test)]
use self::{
    attachments::filename,
    forms::format_id,
    walker::{Walker, MAX_DEPTH, MAX_ENTRIES},
};
#[cfg(test)]
use lopdf::{Dictionary, Document, Object, ObjectId};
#[cfg(test)]
mod tests;
