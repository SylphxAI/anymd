use std::collections::HashSet;

use lopdf::{Dictionary, Object, ObjectId};

use super::walker::{Walker, MAX_ENTRIES, MAX_STRING_BYTES, MAX_TEXT_BYTES, MAX_TREE_DEPTH};

#[derive(Clone)]
struct LabelRange {
    start: u32,
    style: Option<Vec<u8>>,
    prefix: String,
    first: u32,
}

pub(super) fn extract_page_labels(
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
        for pair in nums.as_chunks::<2>().0.iter() {
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
