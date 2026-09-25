//! Deterministic OCR-word table extraction and selectable/OCR evidence merge.
//!
//! This is the provider-neutral projection of the immutable TypeScript
//! v3.0.14 table contract. OCR prose alone is intentionally insufficient:
//! only normalized non-empty words with valid PDF-coordinate boxes can form
//! tables.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{json, Value};

use crate::ocr_fusion::OcrPage;

const Y_TOLERANCE: f64 = 5.0;
const COLUMN_GAP_THRESHOLD: f64 = 15.0;
const MIN_ROWS: usize = 2;
const MIN_COLS: usize = 2;
const MIN_ROW_ITEMS: usize = 2;
const PAGE_EDGE_CONTINUATION_BOTTOM_Y: f64 = 120.0;
const PAGE_EDGE_CONTINUATION_TOP_Y: f64 = 500.0;
const COLUMN_GEOMETRY_TOLERANCE: f64 = 24.0;
const CONTINUATION_MIN_GEOMETRY_SIMILARITY: f64 = 0.8;
const MAX_OCR_TABLE_PAGES: usize = 20;
const MAX_OCR_WORDS_PER_PAGE: usize = 50_000;
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

#[derive(Clone, Debug)]
struct TextItem {
    text: String,
    x: f64,
    y: f64,
    box_: Box2d,
}

#[derive(Clone, Debug)]
struct TextRow {
    y: f64,
    items: Vec<TextItem>,
}

#[derive(Clone, Debug)]
struct Region {
    rows: Vec<TextRow>,
    columns: Vec<f64>,
}

fn cmp_f64(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

fn round_ratio(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn merge_boxes(boxes: impl IntoIterator<Item = Box2d>) -> Option<Box2d> {
    boxes.into_iter().reduce(|left, right| Box2d {
        left: left.left.min(right.left),
        bottom: left.bottom.min(right.bottom),
        right: left.right.max(right.right),
        top: left.top.max(right.top),
    })
}

fn text_items(page: &OcrPage) -> Vec<TextItem> {
    page.words
        .as_deref()
        .unwrap_or_default()
        .iter()
        .take(MAX_OCR_WORDS_PER_PAGE)
        .filter_map(|word| {
            let text = word.text.trim();
            let box_ = Box2d::from_value(word.bounding_box.as_ref()?)?;
            (!text.is_empty()).then(|| TextItem {
                text: text.to_string(),
                x: box_.left,
                y: box_.bottom,
                box_,
            })
        })
        .collect()
}

fn cluster_rows(mut items: Vec<TextItem>) -> Vec<TextRow> {
    items.sort_by(|left, right| cmp_f64(right.y, left.y));
    let Some(first) = items.first().cloned() else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut current = TextRow {
        y: first.y,
        items: vec![first],
    };
    for item in items.into_iter().skip(1) {
        if (current.y - item.y).abs() <= Y_TOLERANCE {
            current.items.push(item);
        } else {
            current
                .items
                .sort_by(|left, right| cmp_f64(left.x, right.x));
            rows.push(current);
            current = TextRow {
                y: item.y,
                items: vec![item],
            };
        }
    }
    current
        .items
        .sort_by(|left, right| cmp_f64(left.x, right.x));
    rows.push(current);
    rows
}

fn detect_columns(rows: &[TextRow]) -> Vec<f64> {
    let mut positions = rows
        .iter()
        .flat_map(|row| row.items.iter().map(|item| item.x))
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| cmp_f64(*left, *right));
    let Some(first) = positions.first().copied() else {
        return Vec::new();
    };
    let mut columns = vec![first];
    for pair in positions.windows(2) {
        if pair[1] - pair[0] >= COLUMN_GAP_THRESHOLD {
            columns.push(pair[1]);
        }
    }
    columns
}

fn column_for(x: f64, columns: &[f64]) -> usize {
    columns
        .iter()
        .rposition(|boundary| x >= *boundary - COLUMN_GAP_THRESHOLD / 2.0)
        .unwrap_or(0)
}

fn identify_regions(rows: Vec<TextRow>) -> Vec<Region> {
    let candidates = rows
        .into_iter()
        .filter(|row| row.items.len() >= MIN_ROW_ITEMS)
        .collect::<Vec<_>>();
    if candidates.len() < MIN_ROWS {
        return Vec::new();
    }
    let columns = detect_columns(&candidates);
    if columns.len() < MIN_COLS {
        return Vec::new();
    }
    let mut regions = Vec::new();
    let mut current = Vec::new();
    for row in candidates {
        let aligned = row
            .items
            .iter()
            .filter(|item| {
                columns
                    .iter()
                    .any(|boundary| (item.x - boundary).abs() < COLUMN_GAP_THRESHOLD)
            })
            .count();
        if aligned >= MIN_COLS - 1 {
            current.push(row);
        } else {
            if current.len() >= MIN_ROWS {
                regions.push(Region {
                    rows: std::mem::take(&mut current),
                    columns: columns.clone(),
                });
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= MIN_ROWS {
        regions.push(Region {
            rows: current,
            columns,
        });
    }
    regions
}

fn row_spacing_consistency(rows: &[TextRow]) -> f64 {
    if rows.len() < 3 {
        return if rows.len() >= 2 { 1.0 } else { 0.0 };
    }
    let spacings = rows
        .windows(2)
        .map(|pair| (pair[0].y - pair[1].y).abs())
        .collect::<Vec<_>>();
    let average = spacings.iter().sum::<f64>() / spacings.len() as f64;
    if average <= 0.0 {
        return 0.0;
    }
    let variance = spacings
        .iter()
        .map(|spacing| (spacing - average).powi(2))
        .sum::<f64>()
        / spacings.len() as f64;
    round_ratio((1.0 - variance.sqrt() / average).max(0.0))
}

fn row_alignment(rows: &[TextRow], columns: &[f64]) -> f64 {
    if rows.is_empty() || columns.is_empty() {
        return 0.0;
    }
    let total = rows
        .iter()
        .map(|row| {
            let occupied = row
                .items
                .iter()
                .map(|item| column_for(item.x, columns))
                .collect::<BTreeSet<_>>()
                .len();
            (occupied as f64 / columns.len() as f64).min(1.0)
        })
        .sum::<f64>();
    round_ratio(total / rows.len() as f64)
}

fn confidence(rows: &[TextRow], columns: &[f64]) -> f64 {
    if rows.len() < MIN_ROWS || columns.len() < MIN_COLS {
        return 0.0;
    }
    let mut score = 0.0;
    let mut checks = 0usize;
    for row in rows {
        score += row
            .items
            .iter()
            .map(|item| column_for(item.x, columns))
            .collect::<BTreeSet<_>>()
            .len() as f64
            / columns.len() as f64;
        checks += 1;
    }
    let spacings = rows
        .windows(2)
        .map(|pair| (pair[0].y - pair[1].y).abs())
        .collect::<Vec<_>>();
    if !spacings.is_empty() {
        let average = spacings.iter().sum::<f64>() / spacings.len() as f64;
        let variance = spacings
            .iter()
            .map(|spacing| (spacing - average).powi(2))
            .sum::<f64>()
            / spacings.len() as f64;
        score += if average > 0.0 {
            (1.0 - variance.sqrt() / average).max(0.0)
        } else {
            0.0
        };
        checks += 1;
    }
    (score / checks as f64).min(1.0)
}

fn infer_span(box_: Option<Box2d>, column: usize, columns: &[f64]) -> usize {
    let Some(box_) = box_ else { return 1 };
    let mut span = 1;
    for boundary in columns.iter().skip(column + 1) {
        if box_.right >= *boundary - COLUMN_GAP_THRESHOLD / 2.0 {
            span += 1;
        } else {
            break;
        }
    }
    span.clamp(1, columns.len() - column)
}

fn quality(rows: &[TextRow], cells: &[Value], columns: &[f64], confidence: f64) -> Value {
    let non_empty = cells
        .iter()
        .filter(|cell| {
            cell.get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
        })
        .count();
    let boxed = cells
        .iter()
        .filter(|cell| cell.get("bounding_box").is_some())
        .count();
    let inferred = cells
        .iter()
        .filter(|cell| cell.get("inferred") == Some(&Value::Bool(true)))
        .count();
    let missing = cells.len().saturating_sub(non_empty);
    let merged = cells
        .iter()
        .filter(|cell| cell.get("colSpan").and_then(Value::as_u64).unwrap_or(1) > 1)
        .count();
    let ratio = |count: usize| {
        if cells.is_empty() {
            0.0
        } else {
            round_ratio(count as f64 / cells.len() as f64)
        }
    };
    let alignment = row_alignment(rows, columns);
    let spacing = row_spacing_consistency(rows);
    let mut signals = vec![if missing == 0 {
        "complete_grid"
    } else {
        "missing_cells"
    }];
    let mut warnings = Vec::new();
    if missing > 0 {
        warnings
            .push("Detected empty inferred cells; table may contain sparse or merged structure.");
    }
    if merged > 0 {
        signals.push("merged_cell_candidates");
        warnings
            .push("Detected cells whose text boxes cross column boundaries; spans are inferred.");
    }
    if boxed < cells.len() {
        signals.push("incomplete_cell_geometry");
        warnings.push("Some table cells lack bounding boxes; verify the table with region crops when cell-level evidence matters.");
    }
    if spacing < 0.75 {
        signals.push("irregular_row_spacing");
        warnings.push("Row spacing is irregular; verify the table with visual evidence when precision matters.");
    }
    if confidence < 0.65 {
        signals.push("low_confidence");
        warnings.push("Table detector confidence is low; use region crops or page rendering for verification.");
    }
    let mut value = json!({
        "completeness": round_ratio(ratio(non_empty) * alignment),
        "nonEmptyCellRatio": ratio(non_empty),
        "cellBoundingBoxCoverage": ratio(boxed),
        "inferredCellRatio": ratio(inferred),
        "rowAlignment": alignment,
        "rowSpacingConsistency": spacing,
        "cellBoundingBoxCount": boxed,
        "inferredCellCount": inferred,
        "missingCellCount": missing,
        "mergedCellCandidateCount": merged,
        "signals": signals,
    });
    if !warnings.is_empty() {
        value["warnings"] = json!(warnings);
    }
    value
}

fn table_for_region(page: &OcrPage, table_index: usize, region: Region) -> Option<Value> {
    let score = confidence(&region.rows, &region.columns);
    if score < 0.3 {
        return None;
    }
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    for (row_index, row) in region.rows.iter().enumerate() {
        let mut by_column = vec![Vec::<&TextItem>::new(); region.columns.len()];
        for item in &row.items {
            by_column[column_for(item.x, &region.columns)].push(item);
        }
        let mut values = Vec::with_capacity(region.columns.len());
        for (column, items) in by_column.into_iter().enumerate() {
            let text = items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let box_ = merge_boxes(items.iter().map(|item| item.box_));
            let span = infer_span(box_, column, &region.columns);
            let mut cell = json!({
                "text": text,
                "rowIndex": row_index,
                "colIndex": column,
                "rowSpan": 1,
                "colSpan": span,
                "isHeader": row_index == 0,
                "inferred": items.is_empty(),
            });
            if let Some(box_) = box_ {
                cell["bounding_box"] = box_.to_value();
            }
            values.push(text);
            cells.push(cell);
        }
        rows.push(values);
    }
    let box_ = merge_boxes(
        cells
            .iter()
            .filter_map(|cell| Box2d::from_value(cell.get("bounding_box")?)),
    );
    let rounded = round_ratio(score);
    let mut table = json!({
        "page": page.page,
        "tableIndex": table_index,
        "rows": rows,
        "cells": cells,
        "rowCount": region.rows.len(),
        "colCount": region.columns.len(),
        "confidence": rounded,
        "provenance": {
            "source": "ocr_text_layer",
            "engine": "external-command",
            "ocr_source_render_evidence_id": page.source_render_evidence_id,
        },
    });
    if let Some(box_) = box_ {
        table["bounding_box"] = box_.to_value();
    }
    table["quality"] = quality(&region.rows, &cells, &region.columns, rounded);
    Some(table)
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

fn table_key_from_id(id: &str) -> Option<(u64, u64)> {
    let value = id.strip_prefix('p')?;
    let (page, table) = value.split_once("-table-")?;
    let table = table.parse::<u64>().ok()?.checked_sub(1)?;
    Some((page.parse().ok()?, table))
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

fn header(table: &Value) -> BTreeSet<String> {
    table
        .get("rows")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn header_similarity(left: &Value, right: &Value) -> f64 {
    let left = header(left);
    let right = header(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    left.intersection(&right).count() as f64 / left.len().max(right.len()) as f64
}

fn column_anchors(table: &Value) -> Option<Vec<f64>> {
    let count = table.get("colCount")?.as_u64()? as usize;
    let cells = table.get("cells")?.as_array()?;
    (0..count)
        .map(|column| {
            cells
                .iter()
                .filter(|cell| {
                    cell.get("colIndex").and_then(Value::as_u64) == Some(column as u64)
                        && cell.get("inferred") != Some(&Value::Bool(true))
                })
                .filter_map(|cell| Box2d::from_value(cell.get("bounding_box")?))
                .map(|box_| box_.left)
                .min_by(|left, right| cmp_f64(*left, *right))
        })
        .collect()
}

fn continuation_evidence(left: &Value, right: &Value) -> Option<(f64, Vec<&'static str>)> {
    if left.get("colCount") != right.get("colCount") {
        return None;
    }
    let similarity = header_similarity(left, right);
    if similarity >= 0.6 {
        return Some((
            round_ratio(0.55 + similarity * 0.4),
            vec!["same_column_count", "repeated_header_candidate"],
        ));
    }
    let left_anchors = column_anchors(left)?;
    let right_anchors = column_anchors(right)?;
    if left_anchors.len() != right_anchors.len() || left_anchors.is_empty() {
        return None;
    }
    let geometry = round_ratio(
        left_anchors
            .iter()
            .zip(right_anchors)
            .map(|(left, right)| (1.0 - (left - right).abs() / COLUMN_GEOMETRY_TOLERANCE).max(0.0))
            .sum::<f64>()
            / left_anchors.len() as f64,
    );
    let (Some(left_box), Some(right_box)) = (table_box(left), table_box(right)) else {
        return None;
    };
    if geometry < CONTINUATION_MIN_GEOMETRY_SIMILARITY
        || left_box.bottom > PAGE_EDGE_CONTINUATION_BOTTOM_Y
        || right_box.top < PAGE_EDGE_CONTINUATION_TOP_Y
    {
        return None;
    }
    Some((
        round_ratio((0.58 + geometry * 0.25 + 0.12).min(0.95)),
        vec![
            "same_column_count",
            "column_geometry_match",
            "page_edge_continuation_candidate",
            "non_repeated_header_candidate",
        ],
    ))
}

fn add_quality_signal(table: &mut Value, signal: &str) {
    let Some(signals) = table
        .get_mut("quality")
        .and_then(|quality| quality.get_mut("signals"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    let signal = json!(signal);
    if !signals.contains(&signal) {
        signals.push(signal);
    }
}

fn link_continuations(tables: &mut [Value]) {
    tables.sort_by_key(|table| (table_page(table), table_index(table)));
    for index in 0..tables.len().saturating_sub(1) {
        let (left, right) = tables.split_at_mut(index + 1);
        let current = &mut left[index];
        let next = &mut right[0];
        if table_page(next) != table_page(current) + 1 {
            continue;
        }
        let Some((confidence, signals)) = continuation_evidence(current, next) else {
            continue;
        };
        let current_id = table_id(current);
        let next_id = table_id(next);
        let group = format!("table-continuation-{current_id}-{next_id}");
        let previous_id = current
            .pointer("/continuation/previousTableId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let following_id = next
            .pointer("/continuation/nextTableId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut current_continuation = json!({
            "groupId": group,
            "role": if previous_id.is_some() { "continues" } else { "starts" },
            "nextTableId": next_id,
            "confidence": confidence,
            "signals": signals,
        });
        if let Some(previous_id) = previous_id {
            current_continuation["previousTableId"] = json!(previous_id);
        }
        current["continuation"] = current_continuation;
        let mut next_continuation = json!({
            "groupId": group,
            "role": if following_id.is_some() { "continues" } else { "ends" },
            "previousTableId": current_id,
            "confidence": confidence,
            "signals": signals,
        });
        if let Some(following_id) = following_id {
            next_continuation["nextTableId"] = json!(following_id);
        }
        next["continuation"] = next_continuation;
        add_quality_signal(current, "multi_page_continuation_candidate");
        add_quality_signal(next, "multi_page_continuation_candidate");
    }
}

/// Extract TS v3.0.14-compatible tables from normalized OCR word boxes.
pub fn extract_ocr_tables(ocr_pages: &[OcrPage]) -> Value {
    let mut tables = ocr_pages
        .iter()
        .take(MAX_OCR_TABLE_PAGES)
        .flat_map(|page| {
            identify_regions(cluster_rows(text_items(page)))
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

fn rebase_continuation(
    table: &mut Value,
    old_id: &str,
    new_id: &str,
    id_map: &HashMap<(u64, u64), String>,
) {
    let Some(continuation) = table.get_mut("continuation").and_then(Value::as_object_mut) else {
        return;
    };
    let remap = |value: Option<&Value>| {
        value.and_then(Value::as_str).map(|id| {
            table_key_from_id(id)
                .and_then(|key| id_map.get(&key).cloned())
                .unwrap_or_else(|| id.to_string())
        })
    };
    let previous = remap(continuation.get("previousTableId"));
    let next = remap(continuation.get("nextTableId"));
    if let Some(previous) = previous.as_ref() {
        continuation.insert("previousTableId".into(), json!(previous));
    }
    if let Some(next) = next.as_ref() {
        continuation.insert("nextTableId".into(), json!(next));
    }
    let group = if let Some(previous) = previous {
        format!("table-continuation-{previous}-{new_id}")
    } else if let Some(next) = next {
        format!("table-continuation-{new_id}-{next}")
    } else {
        continuation
            .get("groupId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .replace(old_id, new_id)
    };
    continuation.insert("groupId".into(), json!(group));
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
mod tests {
    use super::*;
    use crate::ocr_fusion::OcrWord;

    fn word(text: &str, left: f64, bottom: f64, right: f64, top: f64) -> OcrWord {
        OcrWord {
            text: text.into(),
            confidence: Some(0.9),
            bounding_box: Some(json!({
                "left": left,
                "bottom": bottom,
                "right": right,
                "top": top,
            })),
        }
    }

    fn page(page: u32, words: Vec<OcrWord>) -> OcrPage {
        OcrPage {
            page,
            text: words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            confidence: Some(0.92),
            words: Some(words),
            language: Some("eng".into()),
            provider: "command".into(),
            source_render_evidence_id: format!("page-{page}-render-scale-2"),
            source_render_scale: Some(2.0),
            source_render_width: Some(1224),
            source_render_height: Some(1584),
            provenance: json!({"engine":"external-command","source":"ocr-provider"}),
            warnings: None,
        }
    }

    fn ocr_table_page(words_left: f64, page_number: u32) -> OcrPage {
        page(
            page_number,
            vec![
                word("Metric", words_left, 700.0, words_left + 48.0, 710.0),
                word(
                    "Value",
                    words_left + 120.0,
                    700.0,
                    words_left + 162.0,
                    710.0,
                ),
                word("Revenue", words_left, 680.0, words_left + 60.0, 690.0),
                word("24%", words_left + 120.0, 680.0, words_left + 144.0, 690.0),
            ],
        )
    }

    #[test]
    fn extracts_boxed_two_by_two_with_cells_provenance_and_quality() {
        let result = extract_ocr_tables(&[ocr_table_page(40.0, 1)]);
        let table = &result[0];
        assert_eq!(
            table["rows"],
            json!([["Metric", "Value"], ["Revenue", "24%"]])
        );
        assert_eq!(table["rowCount"], 2);
        assert_eq!(table["colCount"], 2);
        assert_eq!(table["cells"].as_array().map(Vec::len), Some(4));
        assert_eq!(
            table["bounding_box"],
            json!({"left":40.0,"bottom":680.0,"right":202.0,"top":710.0})
        );
        assert_eq!(table["provenance"]["source"], "ocr_text_layer");
        assert_eq!(table["quality"]["cellBoundingBoxCoverage"], 1.0);
        assert_eq!(table["quality"]["signals"], json!(["complete_grid"]));
    }

    #[test]
    fn unboxed_words_and_invalid_boxes_do_not_form_tables() {
        let mut invalid = word("bad", 1.0, 1.0, 2.0, 2.0);
        invalid.bounding_box = Some(json!({"left":1,"bottom":1,"right":1,"top":2}));
        let unboxed = OcrWord {
            text: "plain".into(),
            confidence: None,
            bounding_box: None,
        };
        assert_eq!(
            extract_ocr_tables(&[page(1, vec![invalid, unboxed])]),
            json!([])
        );
    }

    #[test]
    fn removes_overlap_but_keeps_and_reindexes_distinct_ocr_table() {
        let selectable = json!([
            {
                "page":1,
                "tableIndex":0,
                "rows":[["existing"]],
                "bounding_box":{"left":39,"bottom":679,"right":203,"top":711}
            },
            {
                "page":1,
                "tableIndex":4,
                "rows":[["later"]],
                "bounding_box":{"left":500,"bottom":100,"right":550,"top":120}
            }
        ]);
        let result = merge_ocr_tables(
            &[ocr_table_page(40.0, 1), ocr_table_page(300.0, 1)],
            &selectable,
        );
        let tables = result.as_array().expect("tables");
        assert_eq!(tables.len(), 3);
        assert_eq!(
            tables.iter().map(table_index).collect::<Vec<_>>(),
            vec![0, 4, 5]
        );
        assert_eq!(tables[2]["provenance"]["source"], "ocr_text_layer");
    }

    #[test]
    fn sorts_by_page_then_index_and_rebases_continuation_ids() {
        let selectable = json!([{
            "page":1,
            "tableIndex":0,
            "rows":[["selectable"]],
            "bounding_box":{"left":500,"bottom":100,"right":550,"top":120}
        }]);
        let result = merge_ocr_tables(
            &[ocr_table_page(300.0, 2), ocr_table_page(300.0, 1)],
            &selectable,
        );
        let tables = result.as_array().expect("tables");
        assert_eq!(
            tables
                .iter()
                .map(|table| (table_page(table), table_index(table)))
                .collect::<Vec<_>>(),
            vec![(1, 0), (1, 1), (2, 0)]
        );
        assert_eq!(tables[1]["continuation"]["nextTableId"], "p2-table-1");
        assert_eq!(tables[2]["continuation"]["previousTableId"], "p1-table-2");
        assert_eq!(
            tables[1]["continuation"]["groupId"],
            "table-continuation-p1-table-2-p2-table-1"
        );
    }

    #[test]
    fn non_array_selectable_input_is_an_empty_collection() {
        let result = merge_ocr_tables(&[ocr_table_page(40.0, 2)], &Value::Null);
        assert_eq!(result[0]["page"], 2);
        assert_eq!(result[0]["tableIndex"], 0);
    }
}
