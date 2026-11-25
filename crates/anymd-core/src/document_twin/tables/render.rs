//! Convert admitted table regions into cells, geometry, and quality evidence.

use std::collections::HashSet;

use serde_json::{json, Value};

use super::super::text_layer::merge_text_boxes;
use super::grid::{TableTextRow, TABLE_COLUMN_GAP};
use super::table_round;
use crate::text_index::TextBoundingBox;

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

pub(super) fn render_table(
    page: u32,
    table_index: usize,
    region: &[TableTextRow<'_>],
    boundaries: &[f64],
) -> Option<Value> {
    let confidence = table_confidence(region, boundaries);
    if confidence < 0.3 {
        return None;
    }
    let confidence = table_round(confidence);
    let mut output_rows = Vec::new();
    let mut cells = Vec::new();
    let mut table_box = None;
    for (row_index, row) in region.iter().enumerate() {
        let mut text_parts = vec![Vec::<&str>::new(); boundaries.len()];
        let mut cell_boxes = vec![None::<TextBoundingBox>; boundaries.len()];
        for item in &row.items {
            let col_index = table_column_index(item.x, boundaries);
            text_parts[col_index].push(item.text);
            cell_boxes[col_index] = merge_text_boxes(cell_boxes[col_index], item.bounding_box);
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
    let alignment = table_row_alignment(region, boundaries);
    let spacing = table_spacing_consistency(region);
    let mut signals = Vec::new();
    let mut quality_warnings = Vec::new();
    if missing == 0 {
        signals.push("complete_grid");
    } else {
        signals.push("missing_cells");
        quality_warnings
            .push("Detected empty inferred cells; table may contain sparse or merged structure.");
    }
    if merged > 0 {
        signals.push("merged_cell_candidates");
        quality_warnings
            .push("Detected cells whose text boxes cross column boundaries; spans are inferred.");
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
        "page": page,
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
    Some(table)
}
