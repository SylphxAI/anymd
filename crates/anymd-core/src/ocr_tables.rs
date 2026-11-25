//! Deterministic OCR-word table extraction and selectable/OCR evidence merge.
//!
//! This is the provider-neutral projection of the immutable TypeScript
//! v3.0.14 table contract. OCR prose alone is intentionally insufficient:
//! only normalized non-empty words with valid PDF-coordinate boxes can form
//! tables.

mod continuation;
mod grid;
mod projection;

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use crate::ocr_fusion::OcrPage;

use continuation::{link_continuations, rebase_continuation};
use grid::regions_for_page;
use projection::table_for_region;

const MAX_OCR_TABLE_PAGES: usize = 20;
const MAX_OCR_TABLES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Box2d {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}

impl Box2d {
    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let box_ = Self {
            left: object.get("left")?.as_f64()?,
            bottom: object.get("bottom")?.as_f64()?,
            right: object.get("right")?.as_f64()?,
            top: object.get("top")?.as_f64()?,
        };
        ([box_.left, box_.bottom, box_.right, box_.top]
            .into_iter()
            .all(f64::is_finite)
            && box_.right > box_.left
            && box_.top > box_.bottom)
            .then_some(box_)
    }

    fn area(self) -> f64 {
        (self.right - self.left).max(0.0) * (self.top - self.bottom).max(0.0)
    }

    fn to_value(self) -> Value {
        json!({
            "left": self.left,
            "bottom": self.bottom,
            "right": self.right,
            "top": self.top,
        })
    }
}

fn cmp_f64(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

fn table_page(table: &Value) -> u64 {
    table.get("page").and_then(Value::as_u64).unwrap_or(0)
}

fn table_index(table: &Value) -> u64 {
    table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0)
}

fn table_id(table: &Value) -> String {
    format!("p{}-table-{}", table_page(table), table_index(table) + 1)
}

fn table_box(table: &Value) -> Option<Box2d> {
    Box2d::from_value(table.get("bounding_box")?)
}

fn overlap_ratio(left: &Value, right: &Value) -> f64 {
    let (Some(left), Some(right)) = (table_box(left), table_box(right)) else {
        return 0.0;
    };
    let intersection = (left.right.min(right.right) - left.left.max(right.left)).max(0.0)
        * (left.top.min(right.top) - left.bottom.max(right.bottom)).max(0.0);
    let denominator = left.area().min(right.area());
    if denominator > 0.0 {
        intersection / denominator
    } else {
        0.0
    }
}

/// Extract TS v3.0.14-compatible tables from normalized OCR word boxes.
pub fn extract_ocr_tables(ocr_pages: &[OcrPage]) -> Value {
    let mut tables = ocr_pages
        .iter()
        .take(MAX_OCR_TABLE_PAGES)
        .flat_map(|page| {
            regions_for_page(page)
                .into_iter()
                .enumerate()
                .filter_map(|(index, region)| table_for_region(page, index, region))
                .collect::<Vec<_>>()
        })
        .take(MAX_OCR_TABLES)
        .collect::<Vec<_>>();
    link_continuations(&mut tables);
    Value::Array(tables)
}

/// Merge selectable table JSON with tables derived from normalized OCR words.
///
/// Non-array selectable input is treated as an empty table collection. Existing
/// selectable values are preserved byte-for-JSON-value; only OCR tables are
/// deduplicated, reindexed, and continuation-rebased.
pub fn merge_ocr_tables(ocr_pages: &[OcrPage], selectable_tables: &Value) -> Value {
    let selectable = selectable_tables.as_array().cloned().unwrap_or_default();
    let extracted = extract_ocr_tables(ocr_pages);
    let mut ocr = extracted.as_array().cloned().unwrap_or_default();
    ocr.retain(|candidate| {
        !selectable.iter().any(|table| {
            table_page(table) == table_page(candidate) && overlap_ratio(table, candidate) >= 0.6
        })
    });

    let mut max_index = BTreeMap::<u64, u64>::new();
    for table in &selectable {
        max_index
            .entry(table_page(table))
            .and_modify(|index| *index = (*index).max(table_index(table)))
            .or_insert_with(|| table_index(table));
    }
    let mut ordinal = BTreeMap::<u64, u64>::new();
    let mut id_map = HashMap::new();
    let mut indexed = Vec::with_capacity(ocr.len());
    for mut table in ocr {
        let page = table_page(&table);
        let old_index = table_index(&table);
        let old_id = table_id(&table);
        let position = ordinal.entry(page).or_default();
        if let Some(base) = max_index.get(&page) {
            table["tableIndex"] = json!(*base + 1 + *position);
        }
        *position += 1;
        id_map.insert((page, old_index), table_id(&table));
        indexed.push((old_id, table));
    }
    for (old_id, table) in &mut indexed {
        let new_id = table_id(table);
        rebase_continuation(table, old_id, &new_id, &id_map);
    }

    let mut merged = selectable;
    merged.extend(indexed.into_iter().map(|(_, table)| table));
    merged.sort_by_key(|table| (table_page(table), table_index(table)));
    Value::Array(merged)
}

#[cfg(test)]
mod tests;
