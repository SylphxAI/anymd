//! Project a detected grid into the table JSON contract.

use serde_json::{json, Value};

use crate::fusion_util::round_ratio;
use crate::ocr_fusion::OcrPage;

use super::grid::{
    column_for, confidence, infer_span, row_alignment, row_spacing_consistency, Region, TextItem,
    TextRow,
};
use super::Box2d;

fn merge_boxes(boxes: impl IntoIterator<Item = Box2d>) -> Option<Box2d> {
    boxes.into_iter().reduce(|left, right| Box2d {
        left: left.left.min(right.left),
        bottom: left.bottom.min(right.bottom),
        right: left.right.max(right.right),
        top: left.top.max(right.top),
    })
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

pub(super) fn table_for_region(
    page: &OcrPage,
    table_index: usize,
    region: Region,
) -> Option<Value> {
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
