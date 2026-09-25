use std::collections::{HashSet, VecDeque};

use crate::pdfjs_text::decode_pdfjs_text_string;
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::Serialize;

use crate::cos_document::EncryptionFacts;

const MAX_REFERENCE_DEPTH: usize = 64;
const MAX_TREE_DEPTH: usize = 64;
const MAX_OBJECT_WORK: usize = 100_000;
const MAX_ENTRIES: usize = 10_000;
const MAX_STRING_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct CatalogSignals {
    pub page_labels: Option<Vec<String>>,
    pub mark_info: Option<MarkInfo>,
    pub permissions: Option<Vec<String>>,
    pub outline: Option<Vec<OutlineItem>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MarkInfo {
    #[serde(rename = "Marked")]
    marked: bool,
    #[serde(rename = "UserProperties")]
    user_properties: bool,
    #[serde(rename = "Suspects")]
    suspects: bool,
}

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

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
enum Destination {
    Text(String),
    Parts(Vec<DestinationPart>),
    Null,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
enum DestinationPart {
    Integer(i64),
    Real(f64),
    Text(String),
    Name { name: String },
    Reference { num: u32, gen: u16 },
    Null,
}

pub(crate) struct CatalogSignalRequest {
    pub page_labels: bool,
    pub permissions: bool,
    pub outline: bool,
}

pub(crate) fn extract_catalog_signals(
    document: &Document,
    encryption_facts: Option<EncryptionFacts>,
    num_pages: u32,
    request: CatalogSignalRequest,
) -> CatalogSignals {
    let mut output = CatalogSignals::default();
    let Ok(catalog) = document.catalog().cloned() else {
        return output;
    };
    if request.page_labels {
        let mut walker = Walker::new(document);
        output.page_labels = extract_page_labels(&mut walker, &catalog, num_pages);
    }
    if request.permissions {
        output.permissions = extract_permissions(encryption_facts);
        let mut walker = Walker::new(document);
        output.mark_info = extract_mark_info(&mut walker, &catalog);
    }
    if request.outline {
        let mut walker = Walker::new(document);
        output.outline = extract_outline(&mut walker, &catalog);
    }
    output
}

struct Walker<'a> {
    document: &'a Document,
    work: usize,
    entries: usize,
    text_remaining: usize,
    truncated: bool,
    failed: bool,
}

impl<'a> Walker<'a> {
    fn new(document: &'a Document) -> Self {
        Self {
            document,
            work: 0,
            entries: 0,
            text_remaining: MAX_TEXT_BYTES,
            truncated: false,
            failed: false,
        }
    }

    fn resolve_owned(&mut self, value: &Object) -> Option<Object> {
        let mut current = value.clone();
        let mut visited = HashSet::new();
        for _ in 0..MAX_REFERENCE_DEPTH {
            self.work += 1;
            if self.work > MAX_OBJECT_WORK {
                self.truncated = true;
                return None;
            }
            let Object::Reference(id) = current else {
                return Some(current);
            };
            if !visited.insert(id) {
                self.truncated = true;
                return None;
            }
            let Ok(next) = self.document.get_object(id) else {
                self.failed = true;
                return None;
            };
            current = next.clone();
        }
        self.truncated = true;
        None
    }

    fn dict(&mut self, value: &Object) -> Option<Dictionary> {
        self.resolve_owned(value)?.as_dict().ok().cloned()
    }

    fn text(&mut self, value: &Object) -> Option<String> {
        let value = self.resolve_owned(value)?;
        let raw_len = match &value {
            Object::String(bytes, _) | Object::Name(bytes) => bytes.len(),
            _ => return None,
        };
        if raw_len > MAX_STRING_BYTES || raw_len > self.text_remaining {
            self.text_remaining = 0;
            self.truncated = true;
            return None;
        }
        let decoded = match &value {
            Object::Name(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => match decode_pdfjs_text_string(&value) {
                Some(value) => value,
                None => {
                    self.failed = true;
                    return None;
                }
            },
        };
        if decoded.len() > MAX_STRING_BYTES || decoded.len() > self.text_remaining {
            self.text_remaining = 0;
            self.truncated = true;
            return None;
        }
        self.text_remaining -= decoded.len();
        Some(decoded)
    }
}

fn extract_mark_info(walker: &mut Walker<'_>, catalog: &Dictionary) -> Option<MarkInfo> {
    let dict = walker.dict(catalog.get(b"MarkInfo").ok()?)?;
    let mut boolean = |key: &[u8]| {
        dict.get(key)
            .ok()
            .and_then(|value| walker.resolve_owned(value))
            .and_then(|value| value.as_bool().ok())
    };
    let value = MarkInfo {
        marked: boolean(b"Marked").unwrap_or(false),
        user_properties: boolean(b"UserProperties").unwrap_or(false),
        suspects: boolean(b"Suspects").unwrap_or(false),
    };
    (!walker.truncated && !walker.failed).then_some(value)
}

fn extract_permissions(facts: Option<EncryptionFacts>) -> Option<Vec<String>> {
    let facts = facts?;
    let raw = facts.permissions?;
    let bits = (raw as i32) as u32;
    let mut labels = Vec::new();
    for (flag, label) in [
        (4, "print"),
        (8, "modify"),
        (16, "copy"),
        (32, "annotate"),
        (256, "fill_forms"),
        (512, "copy_for_accessibility"),
        (1024, "assemble"),
        (2048, "print_high_quality"),
    ] {
        if bits & flag != 0 {
            labels.push(label.to_string());
        }
    }
    (!labels.is_empty()).then_some(labels)
}

#[derive(Clone)]
struct LabelRange {
    start: u32,
    style: Option<Vec<u8>>,
    prefix: String,
    first: u32,
}

fn extract_page_labels(
    walker: &mut Walker<'_>,
    catalog: &Dictionary,
    num_pages: u32,
) -> Option<Vec<String>> {
    if num_pages as usize > MAX_ENTRIES {
        walker.truncated = true;
        return None;
    }
    let root = catalog.get(b"PageLabels").ok()?.clone();
    let mut ranges = Vec::new();
    let mut processed = HashSet::new();
    let mut invalid = false;
    collect_label_ranges(walker, &root, 0, &mut processed, &mut ranges, &mut invalid);
    if invalid || walker.truncated || walker.failed {
        return None;
    }
    ranges.sort_by_key(|range| range.start);
    ranges.dedup_by_key(|range| range.start);
    let mut labels = Vec::with_capacity(num_pages as usize);
    let default_range = LabelRange {
        start: 0,
        style: None,
        prefix: String::new(),
        first: 1,
    };
    let mut active: &LabelRange = &default_range;
    let mut next = 0usize;
    let mut label_bytes_remaining = MAX_TEXT_BYTES;
    for page_index in 0..num_pages {
        while next < ranges.len() && ranges[next].start <= page_index {
            active = &ranges[next];
            next += 1;
        }
        let range = active;
        let number = range
            .first
            .saturating_add(page_index.saturating_sub(range.start));
        let suffix = format_label(range.style.as_deref(), number)?;
        let label_bytes = range.prefix.len().checked_add(suffix.len())?;
        if label_bytes > MAX_STRING_BYTES || label_bytes > label_bytes_remaining {
            return None;
        }
        label_bytes_remaining -= label_bytes;
        labels.push(format!("{}{}", range.prefix, suffix));
    }
    Some(labels)
}

fn collect_label_ranges(
    walker: &mut Walker<'_>,
    value: &Object,
    depth: usize,
    processed: &mut HashSet<ObjectId>,
    output: &mut Vec<LabelRange>,
    invalid: &mut bool,
) {
    if depth >= MAX_TREE_DEPTH || output.len() >= MAX_ENTRIES || walker.entries >= MAX_ENTRIES {
        walker.truncated = true;
        return;
    }
    walker.entries += 1;
    let id = value.as_reference().ok();
    if let Some(id) = id {
        if !processed.insert(id) {
            walker.truncated = true;
            return;
        }
    }
    let Some(dict) = walker.dict(value) else {
        *invalid = true;
        return;
    };
    if let Ok(value) = dict.get(b"Kids") {
        let Some(kids) = walker
            .resolve_owned(value)
            .and_then(|value| value.as_array().ok().cloned())
        else {
            *invalid = true;
            return;
        };
        if kids.len() > MAX_ENTRIES {
            walker.truncated = true;
            return;
        }
        for kid in &kids {
            collect_label_ranges(walker, kid, depth + 1, processed, output, invalid);
            if walker.truncated {
                return;
            }
        }
        // PDF.js treats a node with Kids as an internal node and ignores a
        // same-node Nums entry.
        return;
    }
    if let Ok(value) = dict.get(b"Nums") {
        let Some(nums) = walker
            .resolve_owned(value)
            .and_then(|value| value.as_array().ok().cloned())
        else {
            *invalid = true;
            return;
        };
        if nums.len() % 2 != 0 || nums.len() / 2 > MAX_ENTRIES.saturating_sub(output.len()) {
            walker.truncated = true;
            return;
        }
        for pair in nums.chunks_exact(2) {
            let Some(start) = walker
                .resolve_owned(&pair[0])
                .and_then(|value| value.as_i64().ok())
                .and_then(|n| u32::try_from(n).ok())
            else {
                *invalid = true;
                continue;
            };
            let Some(spec) = walker.dict(&pair[1]) else {
                *invalid = true;
                continue;
            };
            if let Ok(value) = spec.get(b"Type") {
                let valid = walker
                    .resolve_owned(value)
                    .and_then(|value| value.as_name().ok().map(<[u8]>::to_vec))
                    .is_some_and(|name| name == b"PageLabel");
                if !valid {
                    *invalid = true;
                    continue;
                }
            }
            let style = if let Ok(value) = spec.get(b"S") {
                let Some(style) = walker
                    .resolve_owned(value)
                    .and_then(|value| value.as_name().ok().map(<[u8]>::to_vec))
                else {
                    *invalid = true;
                    continue;
                };
                Some(style)
            } else {
                None
            };
            if style
                .as_deref()
                .is_some_and(|value| !matches!(value, b"D" | b"R" | b"r" | b"A" | b"a"))
            {
                *invalid = true;
                continue;
            }
            let prefix = if let Ok(value) = spec.get(b"P") {
                let Some(prefix) = walker.text(value) else {
                    *invalid = true;
                    continue;
                };
                prefix
            } else {
                String::new()
            };
            let first = if let Ok(value) = spec.get(b"St") {
                let Some(value) = value
                    .as_i64()
                    .ok()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                else {
                    *invalid = true;
                    continue;
                };
                value
            } else {
                1
            };
            output.push(LabelRange {
                start,
                style,
                prefix,
                first,
            });
        }
    }
}

fn format_label(style: Option<&[u8]>, number: u32) -> Option<String> {
    match style {
        None => Some(String::new()),
        Some(b"D") => Some(number.to_string()),
        Some(b"R") => roman(number, false),
        Some(b"r") => roman(number, true),
        Some(b"A") => alphabetic(number, false),
        Some(b"a") => alphabetic(number, true),
        _ => None,
    }
}

fn alphabetic(number: u32, lower: bool) -> Option<String> {
    if number == 0 {
        return None;
    }
    let ch = ((number - 1) % 26) as u8 + if lower { b'a' } else { b'A' };
    let repetitions = ((number - 1) / 26 + 1) as usize;
    if repetitions > MAX_STRING_BYTES {
        return None;
    }
    Some(std::iter::repeat_n(char::from(ch), repetitions).collect())
}

fn roman(mut number: u32, lower: bool) -> Option<String> {
    if number == 0 || number > 3999 {
        return None;
    }
    let mut out = String::new();
    for (value, token) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while number >= value {
            number -= value;
            out.push_str(token);
        }
    }
    Some(if lower { out.to_lowercase() } else { out })
}

fn extract_outline(walker: &mut Walker<'_>, catalog: &Dictionary) -> Option<Vec<OutlineItem>> {
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

fn outline_gotor_url(walker: &mut Walker<'_>, action: &Dictionary) -> Option<String> {
    let file = action.get(b"F").ok()?;
    let mut base = match walker.resolve_owned(file)? {
        Object::String(_, _) | Object::Name(_) => walker.text(file)?,
        Object::Dictionary(dict) => outline_filespec_filename(walker, &dict)?,
        _ => return None,
    };
    if base.is_empty() {
        return None;
    }
    if let Some(remote) = outline_remote_dest(walker, action) {
        if let Some(hash) = base.find('#') {
            base.truncate(hash);
        }
        base.push('#');
        base.push_str(&remote);
    }
    safe_outline_url(walker, &base)
}

fn outline_filespec_filename(walker: &mut Walker<'_>, dict: &Dictionary) -> Option<String> {
    // pdf.js pickPlatformItem order: UF, F, Unix, Mac, DOS.
    for key in [b"UF".as_slice(), b"F", b"Unix", b"Mac", b"DOS"] {
        if let Ok(value) = dict.get(key) {
            if let Some(name) = walker.text(value).filter(|value| !value.is_empty()) {
                return Some(name);
            }
        }
    }
    None
}

fn outline_remote_dest(walker: &mut Walker<'_>, action: &Dictionary) -> Option<String> {
    let value = action.get(b"D").ok()?;
    match walker.resolve_owned(value)? {
        Object::String(_, _) | Object::Name(_) => walker.text(value),
        Object::Array(_) => {
            let dest = normalized_destination(walker, value)?;
            serde_json::to_string(&dest).ok()
        }
        _ => None,
    }
}

fn safe_outline_url(walker: &mut Walker<'_>, value: &str) -> Option<String> {
    let parsed = url::Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https" | "ftp" | "mailto" | "tel") {
        return None;
    }
    let normalized = parsed.to_string();
    if normalized.len() > MAX_STRING_BYTES {
        walker.text_remaining = 0;
        walker.truncated = true;
        return None;
    }
    let expansion = normalized.len().saturating_sub(value.len());
    if expansion > walker.text_remaining {
        walker.text_remaining = 0;
        walker.truncated = true;
        return None;
    }
    walker.text_remaining -= expansion;
    Some(normalized)
}

fn finite_number(value: &Object) -> Option<f64> {
    let n = match value {
        Object::Integer(v) => *v as f64,
        Object::Real(v) => f64::from(*v),
        _ => return None,
    };
    n.is_finite().then_some(n)
}

fn normalized_destination(walker: &mut Walker<'_>, value: &Object) -> Option<Destination> {
    let value = walker.resolve_owned(value)?;
    match &value {
        Object::String(_, _) | Object::Name(_) => walker.text(&value).map(Destination::Text),
        Object::Array(parts) if parts.len() <= 8 => {
            let mut output = Vec::new();
            for part in parts {
                output.push(match part {
                    Object::Integer(v) => DestinationPart::Integer(*v),
                    Object::Real(v) if v.is_finite() => DestinationPart::Real(f64::from(*v)),
                    Object::String(_, _) => DestinationPart::Text(walker.text(part)?),
                    Object::Name(_) => DestinationPart::Name {
                        name: walker.text(part)?,
                    },
                    Object::Reference((num, gen)) => DestinationPart::Reference {
                        num: *num,
                        gen: *gen,
                    },
                    Object::Null => DestinationPart::Null,
                    _ => return None,
                });
            }
            Some(Destination::Parts(output))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
