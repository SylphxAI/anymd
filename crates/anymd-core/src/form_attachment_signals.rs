use std::collections::{HashMap, HashSet, VecDeque};

use crate::pdfjs_text::decode_pdfjs_text_string;
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::Serialize;
use serde_json::Value;

const MAX_DEPTH: usize = 64;
const MAX_OBJECTS: usize = 100_000;
const MAX_ENTRIES: usize = 10_000;
const MAX_STRING_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

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

#[derive(Clone, Copy, Default)]
struct Inherited<'a> {
    field_type: Option<&'a Object>,
    flags: Option<&'a Object>,
    value: Option<&'a Object>,
    default_value: Option<&'a Object>,
}

pub(crate) fn extract_form_attachment_signals(
    document: &Document,
    pages: &[(u32, ObjectId)],
    want_forms: bool,
    want_attachments: bool,
) -> FormAttachmentSignals {
    let mut output = FormAttachmentSignals::default();
    let Ok(root) = document.trailer.get(b"Root") else {
        return output;
    };
    if want_forms {
        let mut walker = Walker::new(document);
        output.form_fields = walker
            .dict(root)
            .and_then(|catalog| extract_forms(&mut walker, catalog, pages));
        #[cfg(test)]
        {
            output.form_materialized_array_items = walker.materialized_array_items;
            output.form_annotation_materialized_array_items =
                walker.annotation_materialized_array_items;
        }
        if walker.limited {
            output.form_fields = None;
            output.warnings.push("include_form_fields: COS traversal exceeded the bounded form-field limit; the surface was omitted.".into());
        }
    }
    if want_attachments {
        let mut walker = Walker::new(document);
        output.attachments = walker
            .dict(root)
            .and_then(|catalog| extract_attachments(&mut walker, catalog));
        #[cfg(test)]
        {
            output.attachment_materialized_array_items = walker.materialized_array_items;
        }
        if walker.limited {
            output.attachments = None;
            output.warnings.push("include_attachments: COS traversal exceeded the bounded embedded-file limit; the surface was omitted.".into());
        }
    }
    output
}

struct Walker<'a> {
    document: &'a Document,
    work: usize,
    entries: usize,
    text_remaining: usize,
    form_value_nodes_remaining: usize,
    failed: bool,
    limited: bool,
    #[cfg(test)]
    materialized_array_items: usize,
    #[cfg(test)]
    annotation_materialized_array_items: usize,
}

impl<'a> Walker<'a> {
    fn new(document: &'a Document) -> Self {
        Self {
            document,
            work: 0,
            entries: 0,
            text_remaining: MAX_TEXT_BYTES,
            form_value_nodes_remaining: MAX_ENTRIES,
            failed: false,
            limited: false,
            #[cfg(test)]
            materialized_array_items: 0,
            #[cfg(test)]
            annotation_materialized_array_items: 0,
        }
    }

    fn resolve(&mut self, mut value: &'a Object) -> Option<&'a Object> {
        let mut seen = HashSet::new();
        for _ in 0..MAX_DEPTH {
            self.work += 1;
            if self.work > MAX_OBJECTS {
                self.limited = true;
                return None;
            }
            let Object::Reference(id) = value else {
                return Some(value);
            };
            if !seen.insert(*id) {
                self.failed = true;
                return None;
            }
            let Some(next) = self.document.objects.get(id) else {
                self.failed = true;
                return None;
            };
            value = next;
        }
        self.limited = true;
        None
    }

    fn dict(&mut self, value: &'a Object) -> Option<&'a Dictionary> {
        self.resolve(value)?.as_dict().ok()
    }

    fn array_bounded(&mut self, value: &'a Object, max_len: usize) -> Option<Vec<&'a Object>> {
        let values = self.resolve(value)?.as_array().ok()?;
        if values.len() > max_len {
            self.limited = true;
            return None;
        }
        #[cfg(test)]
        {
            self.materialized_array_items += values.len();
        }
        Some(values.iter().collect())
    }

    fn annotation_array_bounded(
        &mut self,
        value: &'a Object,
        max_len: usize,
    ) -> Option<Vec<&'a Object>> {
        let values = self.array_bounded(value, max_len)?;
        #[cfg(test)]
        {
            self.annotation_materialized_array_items += values.len();
        }
        Some(values)
    }

    fn text(&mut self, value: &'a Object) -> Option<String> {
        let value = self.resolve(value)?;
        let bytes = match value {
            Object::String(bytes, _) | Object::Name(bytes) => bytes,
            _ => return None,
        };
        if bytes.len() > MAX_STRING_BYTES || bytes.len() > self.text_remaining {
            self.limited = true;
            return None;
        }
        let text = match value {
            Object::Name(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => decode_pdfjs_text_string(value)?,
        };
        if text.len() > MAX_STRING_BYTES || text.len() > self.text_remaining {
            self.limited = true;
            return None;
        }
        self.text_remaining -= text.len();
        Some(text)
    }
}

fn extract_forms<'a>(
    walker: &mut Walker<'a>,
    catalog: &'a Dictionary,
    pages: &[(u32, ObjectId)],
) -> Option<Vec<FormField>> {
    let acroform = walker.dict(catalog.get(b"AcroForm").ok()?)?;
    let mut raw_node_budget = MAX_ENTRIES;
    let fields = walker.array_bounded(acroform.get(b"Fields").ok()?, raw_node_budget)?;
    raw_node_budget -= fields.len();
    if pages.len() > MAX_ENTRIES {
        walker.limited = true;
        return None;
    }
    let mut page_annotation_budget = MAX_ENTRIES - pages.len();
    let mut admitted_annotations = Vec::new();
    for (page, page_id) in pages {
        let Some(page_value) = walker.document.objects.get(page_id) else {
            continue;
        };
        let Some(page_dict) = walker.dict(page_value) else {
            if walker.failed || walker.limited {
                return None;
            }
            continue;
        };
        let Ok(annots_value) = page_dict.get(b"Annots") else {
            continue;
        };
        let Some(annots) = walker.annotation_array_bounded(annots_value, page_annotation_budget)
        else {
            if walker.failed || walker.limited {
                return None;
            }
            continue;
        };
        page_annotation_budget -= annots.len();
        admitted_annotations.push((*page, annots));
    }
    let page_by_id = pages
        .iter()
        .map(|(page, id)| (*id, *page))
        .collect::<HashMap<_, _>>();
    let mut annotation_pages = HashMap::new();
    for (page, annots) in admitted_annotations {
        for annot in annots {
            if let Ok(id) = annot.as_reference() {
                annotation_pages.insert(id, page);
            }
        }
    }
    let mut output = Vec::new();
    let mut visited = HashSet::new();
    for field in fields {
        if field.as_reference().is_err() {
            continue;
        }
        walk_field(
            walker,
            field,
            0,
            &page_by_id,
            &annotation_pages,
            &mut visited,
            &mut output,
            &mut raw_node_budget,
        );
        if walker.failed || walker.limited {
            return None;
        }
    }
    (!output.is_empty()).then_some(output)
}

/// pdf.js Annotation._constructFieldName: walk Parent chain for T parts.
fn construct_field_name<'a>(walker: &mut Walker<'a>, dict: &'a Dictionary) -> String {
    let has_t = dict.get(b"T").is_ok();
    let has_parent = dict.get(b"Parent").is_ok();
    if !has_t && !has_parent {
        return String::new();
    }
    if !has_parent {
        return dict
            .get(b"T")
            .ok()
            .and_then(|value| walker.text(value))
            .unwrap_or_default();
    }
    let mut parts = Vec::new();
    if let Some(partial) = dict.get(b"T").ok().and_then(|value| walker.text(value)) {
        if !partial.is_empty() {
            parts.push(partial);
        }
    }
    let mut current = dict.get(b"Parent").ok();
    let mut seen = HashSet::new();
    while let Some(parent_value) = current {
        if let Ok(parent_id) = parent_value.as_reference() {
            if !seen.insert(parent_id) {
                break;
            }
        }
        let Some(parent_dict) = walker.dict(parent_value) else {
            break;
        };
        if let Some(partial) = parent_dict
            .get(b"T")
            .ok()
            .and_then(|value| walker.text(value))
        {
            if !partial.is_empty() {
                parts.push(partial);
            }
        }
        current = parent_dict.get(b"Parent").ok();
    }
    parts.reverse();
    parts.join(".")
}

/// pdf.js getInheritableProperty: first own-or-Parent-chain value for key.
fn inheritable_property<'a>(
    walker: &mut Walker<'a>,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Object> {
    if let Ok(value) = dict.get(key) {
        return Some(value);
    }
    let mut current = dict.get(b"Parent").ok();
    let mut seen = HashSet::new();
    while let Some(parent_value) = current {
        if let Ok(parent_id) = parent_value.as_reference() {
            if !seen.insert(parent_id) {
                break;
            }
        }
        let Some(parent_dict) = walker.dict(parent_value) else {
            break;
        };
        if let Ok(value) = parent_dict.get(key) {
            return Some(value);
        }
        current = parent_dict.get(b"Parent").ok();
    }
    None
}

fn inherited_from_parent_chain<'a>(walker: &mut Walker<'a>, dict: &'a Dictionary) -> Inherited<'a> {
    Inherited {
        field_type: inheritable_property(walker, dict, b"FT"),
        flags: inheritable_property(walker, dict, b"Ff"),
        value: inheritable_property(walker, dict, b"V"),
        default_value: inheritable_property(walker, dict, b"DV"),
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_field<'a>(
    walker: &mut Walker<'a>,
    value: &'a Object,
    depth: usize,
    page_by_id: &HashMap<ObjectId, u32>,
    annotation_pages: &HashMap<ObjectId, u32>,
    visited: &mut HashSet<ObjectId>,
    output: &mut Vec<FormField>,
    raw_node_budget: &mut usize,
) {
    if depth >= MAX_DEPTH || output.len() >= MAX_ENTRIES {
        walker.limited = true;
        return;
    }
    let Ok(id) = value.as_reference() else {
        return;
    };
    if !visited.insert(id) {
        return;
    }
    let Some(resolved) = walker.resolve(value) else {
        return;
    };
    let Ok(dict) = resolved.as_dict() else {
        return;
    };
    if dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Link") {
        return;
    }
    let kids = dict
        .get(b"Kids")
        .ok()
        .and_then(|value| walker.array_bounded(value, *raw_node_budget));
    if let Some(kids) = kids.as_ref() {
        *raw_node_budget -= kids.len();
    }
    // Public names follow pdf.js Parent-chain construction, not Kids-path alone.
    let public_name = construct_field_name(walker, dict).trim().to_string();
    if !public_name.is_empty() {
        if kids.is_some() {
            output.push(FormField {
                name: public_name.clone(),
                r#type: None,
                value: None,
                default_value: None,
                page: None,
                id: Some(format_id(id)),
                editable: None,
                bounding_box: None,
            });
        } else {
            let inherited = inherited_from_parent_chain(walker, dict);
            // pdf.js WidgetAnnotation without inheritable FT falls back to base
            // WidgetAnnotation whose getFieldObject() returns null.
            if inherited.field_type.is_some() {
                if let Some(field) = normalize_leaf(
                    walker,
                    dict,
                    Some(id),
                    public_name,
                    &inherited,
                    page_by_id,
                    annotation_pages,
                ) {
                    output.push(field);
                }
            }
        }
    }
    if let Some(kids) = kids {
        for kid in kids {
            walk_field(
                walker,
                kid,
                depth + 1,
                page_by_id,
                annotation_pages,
                visited,
                output,
                raw_node_budget,
            );
        }
    }
}

fn button_has_named_normal_appearance<'a>(walker: &mut Walker<'a>, dict: &'a Dictionary) -> bool {
    // pdf.js ButtonWidgetAnnotation _processCheckBox/_processRadioButton require
    // AP dict and AP/N named-state dict before setting defaultFieldValue = "Off".
    let Ok(ap) = dict.get(b"AP") else {
        return false;
    };
    let Some(ap_dict) = walker.resolve(ap).and_then(|value| value.as_dict().ok()) else {
        return false;
    };
    let Ok(normal) = ap_dict.get(b"N") else {
        return false;
    };
    walker
        .resolve(normal)
        .and_then(|value| value.as_dict().ok())
        .is_some()
}

fn checkbox_named_export_values<'a>(
    walker: &mut Walker<'a>,
    dict: &'a Dictionary,
    field_value: Option<&Value>,
) -> Option<Vec<String>> {
    // pdf.js _processCheckBox exportValues construction from AP/N keys.
    let Ok(ap) = dict.get(b"AP") else {
        return None;
    };
    let ap_dict = walker.resolve(ap).and_then(|value| value.as_dict().ok())?;
    let Ok(normal) = ap_dict.get(b"N") else {
        return None;
    };
    let normal_dict = walker
        .resolve(normal)
        .and_then(|value| value.as_dict().ok())?;
    let mut keys: Vec<String> = normal_dict
        .iter()
        .filter_map(|(k, _)| std::str::from_utf8(k).ok().map(str::to_string))
        .collect();
    let yes = match field_value {
        Some(Value::String(s)) if !s.is_empty() && s != "Off" => s.clone(),
        _ => "Yes".to_string(),
    };
    if keys.is_empty() {
        keys.push("Off".into());
        keys.push(yes);
    } else if keys.len() == 1 {
        if keys[0] == "Off" {
            keys.push(yes);
        } else {
            keys.insert(0, "Off".into());
        }
    } else if keys.iter().any(|k| k == &yes) {
        keys.clear();
        keys.push("Off".into());
        keys.push(yes);
    } else {
        let other_yes = keys
            .iter()
            .find(|k| k.as_str() != "Off")
            .cloned()
            .unwrap_or_else(|| "Yes".into());
        keys.clear();
        keys.push("Off".into());
        keys.push(other_yes);
    }
    Some(keys)
}

fn normalize_leaf<'a>(
    walker: &mut Walker<'a>,
    dict: &'a Dictionary,
    id: Option<ObjectId>,
    name: String,
    inherited: &Inherited<'a>,
    page_by_id: &HashMap<ObjectId, u32>,
    annotation_pages: &HashMap<ObjectId, u32>,
) -> Option<FormField> {
    let ft = inherited
        .field_type
        .and_then(|v| walker.resolve(v))
        .and_then(|v| v.as_name().ok().map(<[u8]>::to_vec));
    let flags = inherited
        .flags
        .and_then(|v| walker.resolve(v))
        .and_then(|v| v.as_i64().ok())
        .unwrap_or(0);
    let field_type = match ft.as_deref() {
        Some(b"Tx") => Some("text"),
        Some(b"Btn") if flags & (1 << 16) != 0 => Some("button"),
        Some(b"Btn") if flags & (1 << 15) != 0 => Some("radiobutton"),
        Some(b"Btn") => Some("checkbox"),
        Some(b"Ch") if flags & (1 << 17) != 0 => Some("combobox"),
        Some(b"Ch") => Some("listbox"),
        Some(b"Sig") => Some("signature"),
        _ => None,
    }
    .map(str::to_string);
    let raw_value = inherited.value.map(|v| form_value(walker, v));
    let mut default_value = inherited.default_value.and_then(|v| form_value(walker, v));
    let fallback = default_value.clone().filter(|value| !value.is_null());
    let mut value = raw_value.unwrap_or(fallback);
    match field_type.as_deref() {
        Some("text") => {
            value = Some(match value {
                Some(Value::String(value)) => Value::String(value),
                _ => Value::String(String::new()),
            });
            if default_value.as_ref().is_none_or(Value::is_null) {
                default_value = Some(Value::String(String::new()));
            }
        }
        Some("checkbox" | "radiobutton" | "button") => {
            // pdf.js _processCheckBox: when AP/N is a named-state dict, AS string
            // overwrites fieldValue, then exportValues normalization may force Off.
            if field_type.as_deref() == Some("checkbox")
                && button_has_named_normal_appearance(walker, dict)
            {
                if let Some(as_value) = dict.get(b"AS").ok().and_then(|v| form_value(walker, v)) {
                    match &as_value {
                        Value::String(s) if !s.is_empty() => value = Some(as_value),
                        _ => {}
                    }
                }
                if let Some(export_values) =
                    checkbox_named_export_values(walker, dict, value.as_ref())
                {
                    let current = match &value {
                        Some(Value::String(s)) => Some(s.as_str()),
                        _ => None,
                    };
                    if current.is_none_or(|s| !export_values.iter().any(|k| k == s)) {
                        value = Some(Value::String("Off".into()));
                    }
                }
            }
            // pdf.js getFieldObjects keeps non-empty button V/DV arrays as string
            // arrays; only missing/null/empty/non-string non-array values collapse
            // to the public "Off" sentinel.
            value = Some(match value {
                Some(Value::Array(values)) if !values.is_empty() => Value::Array(values),
                Some(Value::String(value)) if !value.is_empty() => Value::String(value),
                _ => Value::String("Off".into()),
            });
            // pdf.js _processCheckBox/_processRadioButton only (not pushbutton):
            // when AP/N is a named-state dict and DV is null, defaultFieldValue
            // becomes "Off". Pushbutton and non-named AP leave default null.
            if default_value.as_ref().is_none_or(Value::is_null) {
                let is_checkbox_or_radio = matches!(
                    field_type.as_deref(),
                    Some("checkbox") | Some("radiobutton")
                );
                if is_checkbox_or_radio && button_has_named_normal_appearance(walker, dict) {
                    default_value = Some(Value::String("Off".into()));
                } else {
                    default_value.get_or_insert(Value::Null);
                }
            }
        }
        Some("listbox" | "combobox") => {
            value = Some(first_choice_value(value));
            default_value.get_or_insert(Value::Null);
        }
        Some("signature") => {
            value = Some(Value::Null);
            default_value = None;
        }
        _ => {}
    }
    let page = dict
        .get(b"P")
        .ok()
        .and_then(|v| v.as_reference().ok())
        .and_then(|id| page_by_id.get(&id).copied())
        .or_else(|| id.and_then(|id| annotation_pages.get(&id).copied()));
    let bounding_box = dict.get(b"Rect").ok().and_then(|v| rect(walker, v));
    Some(FormField {
        name,
        r#type: field_type.clone(),
        value,
        default_value,
        page,
        id: id.map(format_id),
        editable: field_type
            .as_deref()
            .filter(|kind| *kind != "signature")
            .map(|_| flags & 1 == 0),
        bounding_box: (field_type.as_deref() != Some("signature"))
            .then_some(bounding_box)
            .flatten(),
    })
}

fn first_choice_value(value: Option<Value>) -> Value {
    match value {
        Some(Value::Array(values)) => values.into_iter().next().unwrap_or(Value::Null),
        Some(value) => value,
        None => Value::Null,
    }
}

fn form_value<'a>(walker: &mut Walker<'a>, value: &'a Object) -> Option<Value> {
    form_value_at_depth(walker, value, 0)
}

fn form_value_at_depth<'a>(
    walker: &mut Walker<'a>,
    value: &'a Object,
    depth: usize,
) -> Option<Value> {
    if depth >= MAX_DEPTH || walker.form_value_nodes_remaining == 0 {
        walker.limited = true;
        return None;
    }
    walker.form_value_nodes_remaining -= 1;
    let value = walker.resolve(value)?;
    match value {
        Object::String(_, _) | Object::Name(_) => walker.text(value).map(Value::String),
        Object::Array(values) => {
            if values.len() > 256 || values.len() > walker.form_value_nodes_remaining {
                walker.limited = true;
                return None;
            }
            let mut decoded = Vec::new();
            for value in values {
                if let Some(value) = form_value_at_depth(walker, value, depth + 1) {
                    if !value.is_null() {
                        decoded.push(value);
                    }
                }
                if walker.limited {
                    return None;
                }
            }
            Some(if decoded.is_empty() {
                Value::Null
            } else {
                Value::Array(decoded)
            })
        }
        _ => Some(Value::Null),
    }
}

fn rect<'a>(walker: &mut Walker<'a>, value: &'a Object) -> Option<BoxValue> {
    let value = walker.resolve(value)?;
    let values = value.as_array().ok()?;
    if values.len() < 4 {
        return None;
    }
    let mut n = [0.0; 4];
    for (index, value) in values.iter().take(4).enumerate() {
        n[index] = number(walker.resolve(value)?)?;
    }
    Some(BoxValue {
        left: n[0].min(n[2]),
        bottom: n[1].min(n[3]),
        right: n[0].max(n[2]),
        top: n[1].max(n[3]),
    })
}

fn number(value: &Object) -> Option<f64> {
    let n = match value {
        Object::Integer(v) => *v as f64,
        Object::Real(v) => f64::from(*v),
        _ => return None,
    };
    n.is_finite().then_some(n)
}
fn format_id((num, generation): ObjectId) -> String {
    if generation == 0 {
        format!("{num}R")
    } else {
        format!("{num}R{generation}")
    }
}

fn extract_attachments<'a>(
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
            for pair in names.chunks_exact(2) {
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

fn filename<'a>(walker: &mut Walker<'a>, spec: &'a Dictionary) -> Option<String> {
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

#[cfg(test)]
mod tests;
