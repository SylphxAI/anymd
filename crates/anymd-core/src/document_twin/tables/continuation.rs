//! Link adjacent page tables using repeated headers or matching column geometry.

use std::collections::HashSet;

use serde_json::{json, Value};

use super::table_round;

const TABLE_PAGE_EDGE_BOTTOM: f64 = 120.0;
const TABLE_PAGE_EDGE_TOP: f64 = 500.0;
const TABLE_COLUMN_GEOMETRY_TOLERANCE: f64 = 24.0;

fn table_id(table: &Value) -> String {
    format!(
        "p{}-table-{}",
        table.get("page").and_then(Value::as_u64).unwrap_or(0),
        table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0) + 1
    )
}

fn table_header_similarity(left: &Value, right: &Value) -> f64 {
    let header = |table: &Value| {
        table
            .pointer("/rows/0")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|cell| cell.trim().to_lowercase())
            .filter(|cell| !cell.is_empty())
            .collect::<HashSet<_>>()
    };
    let left_header = header(left);
    let right_header = header(right);
    if left_header.is_empty() || right_header.is_empty() {
        return 0.0;
    }
    left_header.intersection(&right_header).count() as f64
        / left_header.len().max(right_header.len()) as f64
}

fn table_geometry_anchors(table: &Value, col_count: usize) -> Option<Vec<f64>> {
    let cells = table.get("cells")?.as_array()?;
    (0..col_count)
        .map(|col_index| {
            cells
                .iter()
                .filter(|cell| {
                    cell.get("colIndex").and_then(Value::as_u64) == Some(col_index as u64)
                        && cell.get("inferred").and_then(Value::as_bool) != Some(true)
                })
                .filter_map(|cell| cell.pointer("/bounding_box/left").and_then(Value::as_f64))
                .reduce(f64::min)
        })
        .collect()
}

fn add_table_quality_signal(table: &mut Value, signal: &str) {
    let Some(signals) = table
        .pointer_mut("/quality/signals")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    if !signals.iter().any(|value| value.as_str() == Some(signal)) {
        signals.push(json!(signal));
    }
}

pub(super) fn link_table_continuations(tables: &mut [Value]) {
    for index in 0..tables.len().saturating_sub(1) {
        let current_page = tables[index]
            .get("page")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let next_page = tables[index + 1]
            .get("page")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let col_count = tables[index]
            .get("colCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        if next_page != current_page + 1
            || tables[index + 1].get("colCount").and_then(Value::as_u64) != Some(col_count as u64)
        {
            continue;
        }

        let header_similarity = table_header_similarity(&tables[index], &tables[index + 1]);
        let evidence = if header_similarity >= 0.6 {
            Some((
                table_round(0.55 + header_similarity * 0.4),
                vec!["same_column_count", "repeated_header_candidate"],
            ))
        } else {
            let geometry = table_geometry_anchors(&tables[index], col_count).and_then(|left| {
                table_geometry_anchors(&tables[index + 1], col_count).map(|right| {
                    table_round(
                        left.iter()
                            .zip(right)
                            .map(|(left, right)| {
                                (1.0 - (left - right).abs() / TABLE_COLUMN_GEOMETRY_TOLERANCE)
                                    .max(0.0)
                            })
                            .sum::<f64>()
                            / col_count.max(1) as f64,
                    )
                })
            });
            let current_bottom = tables[index]
                .pointer("/bounding_box/bottom")
                .and_then(Value::as_f64);
            let next_top = tables[index + 1]
                .pointer("/bounding_box/top")
                .and_then(Value::as_f64);
            geometry
                .filter(|similarity| {
                    *similarity >= 0.8
                        && current_bottom.is_some_and(|bottom| bottom <= TABLE_PAGE_EDGE_BOTTOM)
                        && next_top.is_some_and(|top| top >= TABLE_PAGE_EDGE_TOP)
                })
                .map(|similarity| {
                    (
                        table_round((0.58 + similarity * 0.25 + 0.12).min(0.95)),
                        vec![
                            "same_column_count",
                            "column_geometry_match",
                            "page_edge_continuation_candidate",
                            "non_repeated_header_candidate",
                        ],
                    )
                })
        };
        let Some((confidence, signals)) = evidence else {
            continue;
        };

        let current_id = table_id(&tables[index]);
        let next_id = table_id(&tables[index + 1]);
        let group_id = format!("table-continuation-{current_id}-{next_id}");
        let current_previous = tables[index]
            .pointer("/continuation/previousTableId")
            .cloned();
        let next_following = tables[index + 1]
            .pointer("/continuation/nextTableId")
            .cloned();
        let mut current_continuation = json!({
            "groupId": group_id,
            "role": if current_previous.is_some() { "continues" } else { "starts" },
            "nextTableId": next_id,
            "confidence": confidence,
            "signals": signals,
        });
        if let Some(previous) = current_previous {
            current_continuation["previousTableId"] = previous;
        }
        let mut next_continuation = json!({
            "groupId": group_id,
            "role": if next_following.is_some() { "continues" } else { "ends" },
            "previousTableId": current_id,
            "confidence": confidence,
            "signals": signals,
        });
        if let Some(following) = next_following {
            next_continuation["nextTableId"] = following;
        }
        tables[index]["continuation"] = current_continuation;
        tables[index + 1]["continuation"] = next_continuation;
        add_table_quality_signal(&mut tables[index], "multi_page_continuation_candidate");
        add_table_quality_signal(&mut tables[index + 1], "multi_page_continuation_candidate");
    }
}
