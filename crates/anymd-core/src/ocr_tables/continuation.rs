//! Link tables across pages and rebase their IDs after merging evidence.

use std::collections::{BTreeSet, HashMap};

use serde_json::{json, Value};

use crate::fusion_util::round_ratio;

use super::{cmp_f64, table_box, table_id, table_index, table_page, Box2d};

const PAGE_EDGE_CONTINUATION_BOTTOM_Y: f64 = 120.0;
const PAGE_EDGE_CONTINUATION_TOP_Y: f64 = 500.0;
const COLUMN_GEOMETRY_TOLERANCE: f64 = 24.0;
const CONTINUATION_MIN_GEOMETRY_SIMILARITY: f64 = 0.8;

fn table_key_from_id(id: &str) -> Option<(u64, u64)> {
    let value = id.strip_prefix('p')?;
    let (page, table) = value.split_once("-table-")?;
    let table = table.parse::<u64>().ok()?.checked_sub(1)?;
    Some((page.parse().ok()?, table))
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

pub(super) fn link_continuations(tables: &mut [Value]) {
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

pub(super) fn rebase_continuation(
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
