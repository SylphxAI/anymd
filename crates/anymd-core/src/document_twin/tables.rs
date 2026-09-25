//! Selectable-text table detection, admission limits, and continuation linking.

use std::collections::HashSet;

use serde_json::{json, Value};

use super::text_layer::merge_text_boxes;
use super::PageText;
use crate::text_index::TextBoundingBox;

const TABLE_Y_TOLERANCE: f64 = 5.0;
const TABLE_COLUMN_GAP: f64 = 15.0;
const TABLE_PAGE_EDGE_BOTTOM: f64 = 120.0;
const TABLE_PAGE_EDGE_TOP: f64 = 500.0;
const TABLE_COLUMN_GEOMETRY_TOLERANCE: f64 = 24.0;
const MAX_TABLE_ITEMS_PER_PAGE: usize = 4_096;
const MAX_TABLE_BOUNDARIES_PER_PAGE: usize = 256;
const MAX_TABLE_CELLS_PER_PAGE: usize = 16_384;

#[derive(Clone, Copy)]
struct TableTextItem<'a> {
    text: &'a str,
    x: f64,
    y: f64,
    bounding_box: TextBoundingBox,
}

#[derive(Clone)]
struct TableTextRow<'a> {
    y: f64,
    items: Vec<TableTextItem<'a>>,
}

fn table_text_item(text: &str, bounding_box: Option<TextBoundingBox>) -> Option<TableTextItem<'_>> {
    let text = text.trim();
    let bounding_box = bounding_box?;
    let finite = [
        bounding_box.left,
        bounding_box.bottom,
        bounding_box.right,
        bounding_box.top,
    ]
    .into_iter()
    .all(f64::is_finite);
    (!text.is_empty() && finite).then_some(TableTextItem {
        text,
        x: bounding_box.left,
        y: bounding_box.bottom,
        bounding_box,
    })
}

fn table_round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn table_column_index(x: f64, boundaries: &[f64]) -> usize {
    boundaries
        .iter()
        .rposition(|boundary| x >= *boundary - TABLE_COLUMN_GAP / 2.0)
        .unwrap_or(0)
}

fn table_spacing_consistency(rows: &[TableTextRow<'_>]) -> f64 {
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
        .map(|spacing| (*spacing - average).powi(2))
        .sum::<f64>()
        / spacings.len() as f64;
    table_round((1.0 - variance.sqrt() / average).max(0.0))
}

fn table_row_alignment(rows: &[TableTextRow<'_>], boundaries: &[f64]) -> f64 {
    if rows.is_empty() || boundaries.is_empty() {
        return 0.0;
    }
    let coverage = rows
        .iter()
        .map(|row| {
            let columns = row
                .items
                .iter()
                .map(|item| table_column_index(item.x, boundaries))
                .collect::<HashSet<_>>();
            (columns.len() as f64 / boundaries.len() as f64).min(1.0)
        })
        .sum::<f64>()
        / rows.len() as f64;
    table_round(coverage)
}

fn table_confidence(rows: &[TableTextRow<'_>], boundaries: &[f64]) -> f64 {
    if rows.len() < 2 || boundaries.len() < 2 {
        return 0.0;
    }
    let mut score = 0.0;
    let mut checks = 0usize;
    for row in rows {
        let columns = row
            .items
            .iter()
            .map(|item| table_column_index(item.x, boundaries))
            .collect::<HashSet<_>>();
        score += columns.len() as f64 / boundaries.len() as f64;
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
            .map(|spacing| (*spacing - average).powi(2))
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

fn table_box_value(box_: TextBoundingBox) -> Value {
    json!({
        "left": box_.left,
        "bottom": box_.bottom,
        "right": box_.right,
        "top": box_.top,
    })
}

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

fn link_table_continuations(tables: &mut [Value]) {
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

pub(crate) fn build_tables_with_admission(
    pages: &[PageText],
    page_content_geometry: bool,
) -> (Value, Vec<String>) {
    let mut tables = Vec::new();
    let mut warnings = Vec::new();
    for page in pages {
        // Text segmentation preserves every source PDF text part as a typed run.
        // Tables consume those source parts, matching the pre-segmentation TS
        // extractor boundary and keeping their independent 4,096-item admission
        // reachable even when nearby selectable text joins into one public item.
        let mut items = Vec::new();
        'admission: for item in &page.positioned_items {
            if item.runs.is_empty() {
                if let Some(item) = table_text_item(&item.text, item.bounding_box) {
                    items.push(item);
                }
            } else {
                for run in &item.runs {
                    if let Some(item) = table_text_item(&run.text, run.bounding_box) {
                        items.push(item);
                    }
                    if items.len() > MAX_TABLE_ITEMS_PER_PAGE {
                        break 'admission;
                    }
                }
            }
            if items.len() > MAX_TABLE_ITEMS_PER_PAGE {
                break;
            }
        }
        if items.len() > MAX_TABLE_ITEMS_PER_PAGE {
            warnings.push(format!(
                "Selectable table extraction skipped page {}: spatial grid exceeds the Rust admission limit.",
                page.page
            ));
            continue;
        }
        items.sort_by(|left, right| right.y.total_cmp(&left.y));
        let mut rows = Vec::<TableTextRow<'_>>::new();
        for item in items {
            if let Some(row) = rows.last_mut() {
                if (row.y - item.y).abs() <= TABLE_Y_TOLERANCE {
                    row.items.push(item);
                    continue;
                }
            }
            rows.push(TableTextRow {
                y: item.y,
                items: vec![item],
            });
        }
        for row in &mut rows {
            row.items.sort_by(|left, right| left.x.total_cmp(&right.x));
            if page_content_geometry {
                for index in 0..row.items.len().saturating_sub(1) {
                    let next_x = row.items[index + 1].x;
                    row.items[index].bounding_box.right =
                        row.items[index].bounding_box.right.max(next_x);
                }
            }
        }
        let candidate_rows = rows
            .into_iter()
            .filter(|row| row.items.len() >= 2)
            .collect::<Vec<_>>();
        if candidate_rows.len() < 2 {
            continue;
        }
        let mut x_positions = candidate_rows
            .iter()
            .flat_map(|row| row.items.iter().map(|item| item.x))
            .collect::<Vec<_>>();
        x_positions.sort_by(f64::total_cmp);
        let mut boundaries = x_positions.first().copied().into_iter().collect::<Vec<_>>();
        for pair in x_positions.windows(2) {
            if pair[1] - pair[0] >= TABLE_COLUMN_GAP {
                boundaries.push(pair[1]);
            }
        }
        if boundaries.len() < 2 {
            continue;
        }
        if boundaries.len() > MAX_TABLE_BOUNDARIES_PER_PAGE {
            warnings.push(format!(
                "Selectable table extraction skipped page {}: spatial grid exceeds the Rust admission limit.",
                page.page
            ));
            continue;
        }

        let mut regions = Vec::<Vec<TableTextRow<'_>>>::new();
        let mut current = Vec::new();
        for row in candidate_rows {
            let aligned = row
                .items
                .iter()
                .filter(|item| {
                    boundaries
                        .iter()
                        .any(|boundary| (item.x - boundary).abs() < TABLE_COLUMN_GAP)
                })
                .count();
            if aligned >= boundaries.len() - 1 {
                current.push(row);
            } else if current.len() >= 2 {
                regions.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
        if current.len() >= 2 {
            regions.push(current);
        }

        let page_cell_count = regions.iter().try_fold(0usize, |count, region| {
            region
                .len()
                .checked_mul(boundaries.len())
                .and_then(|cells| count.checked_add(cells))
        });
        if page_cell_count.is_none_or(|count| count > MAX_TABLE_CELLS_PER_PAGE) {
            warnings.push(format!(
                "Selectable table extraction skipped page {}: spatial grid exceeds the Rust admission limit.",
                page.page
            ));
            continue;
        }

        for (table_index, region) in regions.into_iter().enumerate() {
            let confidence = table_confidence(&region, &boundaries);
            if confidence < 0.3 {
                continue;
            }
            let confidence = table_round(confidence);
            let mut output_rows = Vec::new();
            let mut cells = Vec::new();
            let mut table_box = None;
            for (row_index, row) in region.iter().enumerate() {
                let mut text_parts = vec![Vec::<&str>::new(); boundaries.len()];
                let mut cell_boxes = vec![None::<TextBoundingBox>; boundaries.len()];
                for item in &row.items {
                    let col_index = table_column_index(item.x, &boundaries);
                    text_parts[col_index].push(item.text);
                    cell_boxes[col_index] =
                        merge_text_boxes(cell_boxes[col_index], item.bounding_box);
                }
                let mut output_row = Vec::new();
                for col_index in 0..boundaries.len() {
                    let text = text_parts[col_index].join(" ");
                    let box_ = cell_boxes[col_index];
                    let mut col_span = 1usize;
                    if let Some(box_) = box_ {
                        for next_boundary in boundaries.iter().skip(col_index + 1) {
                            if box_.right >= *next_boundary - TABLE_COLUMN_GAP / 2.0 {
                                col_span += 1;
                            } else {
                                break;
                            }
                        }
                        col_span = col_span.min(boundaries.len() - col_index);
                        table_box = merge_text_boxes(table_box, box_);
                    }
                    let mut cell = json!({
                        "text": text,
                        "rowIndex": row_index,
                        "colIndex": col_index,
                        "rowSpan": 1,
                        "colSpan": col_span,
                        "isHeader": row_index == 0,
                        "inferred": text_parts[col_index].is_empty(),
                    });
                    if let Some(box_) = box_ {
                        cell["bounding_box"] = table_box_value(box_);
                    }
                    output_row.push(json!(text));
                    cells.push(cell);
                }
                output_rows.push(Value::Array(output_row));
            }

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
                .filter(|cell| cell.get("inferred").and_then(Value::as_bool) == Some(true))
                .count();
            let merged = cells
                .iter()
                .filter(|cell| cell.get("colSpan").and_then(Value::as_u64).unwrap_or(1) > 1)
                .count();
            let missing = cells.len().saturating_sub(non_empty);
            let non_empty_ratio = table_round(non_empty as f64 / cells.len().max(1) as f64);
            let box_coverage = table_round(boxed as f64 / cells.len().max(1) as f64);
            let inferred_ratio = table_round(inferred as f64 / cells.len().max(1) as f64);
            let alignment = table_row_alignment(&region, &boundaries);
            let spacing = table_spacing_consistency(&region);
            let mut signals = Vec::new();
            let mut quality_warnings = Vec::new();
            if missing == 0 {
                signals.push("complete_grid");
            } else {
                signals.push("missing_cells");
                quality_warnings.push(
                    "Detected empty inferred cells; table may contain sparse or merged structure.",
                );
            }
            if merged > 0 {
                signals.push("merged_cell_candidates");
                quality_warnings.push(
                    "Detected cells whose text boxes cross column boundaries; spans are inferred.",
                );
            }
            if box_coverage < 1.0 {
                signals.push("incomplete_cell_geometry");
                quality_warnings.push("Some table cells lack bounding boxes; verify the table with region crops when cell-level evidence matters.");
            }
            if spacing < 0.75 {
                signals.push("irregular_row_spacing");
                quality_warnings.push("Row spacing is irregular; verify the table with visual evidence when precision matters.");
            }
            if confidence < 0.65 {
                signals.push("low_confidence");
                quality_warnings.push("Table detector confidence is low; use region crops or page rendering for verification.");
            }
            let quality = json!({
                "completeness": table_round(non_empty_ratio * alignment),
                "nonEmptyCellRatio": non_empty_ratio,
                "cellBoundingBoxCoverage": box_coverage,
                "inferredCellRatio": inferred_ratio,
                "rowAlignment": alignment,
                "rowSpacingConsistency": spacing,
                "cellBoundingBoxCount": boxed,
                "inferredCellCount": inferred,
                "missingCellCount": missing,
                "mergedCellCandidateCount": merged,
                "signals": signals,
            });
            let mut quality = quality;
            if !quality_warnings.is_empty() {
                quality["warnings"] = json!(quality_warnings);
            }
            let mut table = json!({
                "page": page.page,
                "tableIndex": table_index,
                "rows": output_rows,
                "cells": cells,
                "rowCount": region.len(),
                "colCount": boundaries.len(),
                "confidence": confidence,
                "provenance": {"source": "selectable_text", "engine": "pdf-reader-core"},
                "quality": quality,
            });
            if let Some(box_) = table_box {
                table["bounding_box"] = table_box_value(box_);
            }
            tables.push(table);
        }
    }
    tables.sort_by_key(|table| {
        (
            table.get("page").and_then(Value::as_u64).unwrap_or(0),
            table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0),
        )
    });
    link_table_continuations(&mut tables);
    (json!(tables), warnings)
}

#[cfg(test)]
pub(crate) fn build_tables(pages: &[PageText]) -> Value {
    build_tables_with_admission(pages, false).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_twin::text_layer::utf16_len;
    use crate::document_twin::*;
    use crate::text_index::{PositionedTextItem, PositionedTextRun};

    #[test]
    fn detects_simple_table() {
        let item = |text: &str, left: f64, bottom: f64| PositionedTextItem {
            text: text.into(),
            bounding_box: Some(TextBoundingBox {
                left,
                bottom,
                right: left + 30.0,
                top: bottom + 10.0,
            }),
            chars: Vec::new(),
            runs: Vec::new(),
        };
        let pages = vec![PageText {
            page: 1,
            text: String::new(),
            positioned_items: vec![
                item("Name", 10.0, 100.0),
                item("Qty", 100.0, 100.0),
                item("Price", 200.0, 100.0),
                item("Apple", 10.0, 80.0),
                item("2", 100.0, 80.0),
                item("1.50", 200.0, 80.0),
                item("Pear", 10.0, 60.0),
                item("3", 100.0, 60.0),
                item("2.00", 200.0, 60.0),
            ],
        }];
        let tables = build_tables(&pages);
        let arr = tables.as_array().unwrap();
        assert!(!arr.is_empty());
        assert_eq!(arr[0]["rowCount"], 3);
        assert_eq!(arr[0]["colCount"], 3);
    }

    #[test]
    fn selectable_tables_consume_preserved_source_runs_after_text_segmentation() {
        let run = |text: &str, start: u32, left: f64, bottom: f64| PositionedTextRun {
            text: text.into(),
            item_char_start: start,
            item_char_end: start + utf16_len(text),
            bounding_box: Some(TextBoundingBox {
                left,
                bottom,
                right: left + 30.0,
                top: bottom + 10.0,
            }),
        };
        let pages = [PageText {
            page: 1,
            text: "NameQtyApple2".into(),
            positioned_items: vec![PositionedTextItem {
                text: "NameQtyApple2".into(),
                bounding_box: Some(TextBoundingBox {
                    left: 20.0,
                    bottom: 80.0,
                    right: 150.0,
                    top: 110.0,
                }),
                chars: Vec::new(),
                runs: vec![
                    run("Name", 0, 20.0, 100.0),
                    run("Qty", 4, 120.0, 100.0),
                    run("Apple", 7, 20.0, 80.0),
                    run("2", 12, 120.0, 80.0),
                ],
            }],
        }];

        let tables = build_tables(&pages);
        assert_eq!(tables[0]["rows"], json!([["Name", "Qty"], ["Apple", "2"]]));
        assert_eq!(tables[0]["rowCount"], 2);
        assert_eq!(tables[0]["colCount"], 2);
    }

    #[test]
    fn selectable_tables_reset_page_indexes_and_link_repeated_headers() {
        let item = |text: &str, left: f64, bottom: f64| PositionedTextItem {
            text: text.into(),
            bounding_box: Some(TextBoundingBox {
                left,
                bottom,
                right: left + 30.0,
                top: bottom + 10.0,
            }),
            chars: Vec::new(),
            runs: Vec::new(),
        };
        let pages = vec![
            PageText {
                page: 1,
                text: String::new(),
                positioned_items: vec![
                    item("Name", 20.0, 100.0),
                    item("Qty", 120.0, 100.0),
                    item("Apple", 20.0, 80.0),
                    item("2", 120.0, 80.0),
                ],
            },
            PageText {
                page: 2,
                text: String::new(),
                positioned_items: vec![
                    item("name", 20.0, 700.0),
                    item("QTY", 120.0, 700.0),
                    item("Pear", 20.0, 680.0),
                    item("3", 120.0, 680.0),
                ],
            },
        ];
        let tables = build_tables(&pages);
        assert_eq!(tables.as_array().unwrap().len(), 2);
        assert_eq!(tables[0]["tableIndex"], 0);
        assert_eq!(tables[1]["tableIndex"], 0);
        assert_eq!(tables[0]["continuation"]["role"], "starts");
        assert_eq!(tables[0]["continuation"]["nextTableId"], "p2-table-1");
        assert_eq!(tables[0]["continuation"]["confidence"], 0.95);
        assert_eq!(tables[1]["continuation"]["role"], "ends");
        assert!(tables[0]["quality"]["signals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|signal| signal == "multi_page_continuation_candidate"));
    }

    #[test]
    fn selectable_tables_match_direct_and_page_content_geometry_paths() {
        let item = |text: &str, left: f64, bottom: f64| PositionedTextItem {
            text: text.into(),
            bounding_box: Some(TextBoundingBox {
                left,
                bottom,
                right: left + 30.0,
                top: bottom + 10.0,
            }),
            chars: Vec::new(),
            runs: Vec::new(),
        };
        let pages = [PageText {
            page: 1,
            text: String::new(),
            positioned_items: vec![
                item("Name", 20.0, 100.0),
                item("Qty", 120.0, 100.0),
                item("Apple", 20.0, 80.0),
                item("2", 120.0, 80.0),
            ],
        }];
        let direct = build_tables_with_admission(&pages, false).0;
        let page_content = build_tables_with_admission(&pages, true).0;
        assert_eq!(direct[0]["quality"]["mergedCellCandidateCount"], 0);
        assert_eq!(direct[0]["cells"][0]["colSpan"], 1);
        assert_eq!(page_content[0]["quality"]["mergedCellCandidateCount"], 2);
        assert_eq!(page_content[0]["cells"][0]["colSpan"], 2);
        assert_eq!(
            page_content[0]["quality"]["warnings"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn selectable_table_admission_accepts_the_exact_item_cap() {
        let positioned_items = (0..MAX_TABLE_ITEMS_PER_PAGE)
            .map(|index| PositionedTextItem {
                text: format!("cell-{index}"),
                bounding_box: Some(TextBoundingBox {
                    left: if index % 2 == 0 { 10.0 } else { 100.0 },
                    bottom: 30_000.0 - (index / 2) as f64 * 10.0,
                    right: if index % 2 == 0 { 30.0 } else { 120.0 },
                    top: 30_010.0 - (index / 2) as f64 * 10.0,
                }),
                chars: Vec::new(),
                runs: Vec::new(),
            })
            .collect();
        let (tables, warnings) = build_tables_with_admission(
            &[PageText {
                page: 6,
                text: String::new(),
                positioned_items,
            }],
            false,
        );
        assert!(warnings.is_empty());
        assert_eq!(tables[0]["rowCount"], 2_048);
        assert_eq!(tables[0]["colCount"], 2);
    }

    #[test]
    fn selectable_table_admission_fails_closed_before_grid_amplification() {
        let positioned_items = (0..=MAX_TABLE_ITEMS_PER_PAGE)
            .map(|index| PositionedTextItem {
                text: format!("cell-{index}"),
                bounding_box: Some(TextBoundingBox {
                    left: if index % 2 == 0 { 10.0 } else { 100.0 },
                    bottom: 700.0 - (index / 2) as f64,
                    right: if index % 2 == 0 { 30.0 } else { 120.0 },
                    top: 710.0 - (index / 2) as f64,
                }),
                chars: Vec::new(),
                runs: Vec::new(),
            })
            .collect();
        let (tables, warnings) = build_tables_with_admission(
            &[PageText {
                page: 7,
                text: String::new(),
                positioned_items,
            }],
            false,
        );
        assert_eq!(tables, json!([]));
        assert_eq!(
            warnings,
            vec!["Selectable table extraction skipped page 7: spatial grid exceeds the Rust admission limit."]
        );
    }
}
