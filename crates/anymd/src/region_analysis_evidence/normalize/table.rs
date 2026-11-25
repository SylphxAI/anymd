//! Table rows, cells, geometry and dimensions.

use serde_json::{json, Map, Value};

use super::scalar::{confidence, normalized_string};

fn positive_integer(value: Option<&Value>) -> Option<u64> {
    value?.as_u64().filter(|value| *value > 0)
}

fn zero_integer(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

fn bounding_box(value: Option<&Value>) -> Option<Value> {
    let object = value?.as_object()?;
    let left = object.get("left")?.as_f64()?;
    let bottom = object.get("bottom")?.as_f64()?;
    let right = object.get("right")?.as_f64()?;
    let top = object.get("top")?.as_f64()?;
    ([left, bottom, right, top]
        .iter()
        .all(|value| value.is_finite())
        && right > left
        && top > bottom)
        .then(|| json!({"left": left, "bottom": bottom, "right": right, "top": top}))
}

fn rows(value: Option<&Value>) -> Option<Vec<Value>> {
    let rows = value?
        .as_array()?
        .iter()
        .filter_map(|row| {
            let cells = row.as_array()?;
            (!cells.is_empty()).then(|| {
                Value::Array(
                    cells
                        .iter()
                        .map(|cell| match cell {
                            Value::Null => Value::String(String::new()),
                            Value::String(value) => Value::String(value.clone()),
                            Value::Number(value) => Value::String(value.to_string()),
                            Value::Bool(value) => Value::String(value.to_string()),
                            _ => Value::String(String::new()),
                        })
                        .collect(),
                )
            })
        })
        .collect::<Vec<_>>();
    (!rows.is_empty()).then_some(rows)
}

fn table_cells(value: Option<&Value>, maximum: usize) -> Option<Vec<Value>> {
    let cells = value?
        .as_array()?
        .iter()
        .filter_map(|cell| {
            let cell = cell.as_object()?;
            let row = zero_integer(cell.get("row_index").or_else(|| cell.get("row")))?;
            let column = zero_integer(cell.get("column_index").or_else(|| cell.get("column")))?;
            let mut output = json!({
                "text": normalized_string(cell.get("text"), maximum).unwrap_or_default(),
                "row_index": row,
                "column_index": column,
            });
            if let Some(value) =
                positive_integer(cell.get("row_span").or_else(|| cell.get("rowspan")))
            {
                output["row_span"] = json!(value);
            }
            if let Some(value) =
                positive_integer(cell.get("column_span").or_else(|| cell.get("colspan")))
            {
                output["column_span"] = json!(value);
            }
            if let Some(value) = confidence(cell.get("confidence")) {
                output["confidence"] = json!(value);
            }
            if let Some(value) = bounding_box(cell.get("bounding_box").or_else(|| cell.get("bbox")))
            {
                output["bounding_box"] = value;
            }
            Some(output)
        })
        .collect::<Vec<_>>();
    (!cells.is_empty()).then_some(cells)
}

pub(super) fn normalize_table(value: Option<&Value>, maximum: usize) -> Option<Value> {
    let candidate = value?.as_object()?;
    let rows = rows(candidate.get("rows"));
    let cells = table_cells(candidate.get("cells"), maximum);
    let row_count = positive_integer(
        candidate
            .get("row_count")
            .or_else(|| candidate.get("rowCount")),
    )
    .or_else(|| rows.as_ref().map(|rows| rows.len() as u64))
    .or_else(|| {
        cells.as_ref().and_then(|cells| {
            cells
                .iter()
                .filter_map(|cell| {
                    Some(
                        cell["row_index"].as_u64()?
                            + cell.get("row_span").and_then(Value::as_u64).unwrap_or(1),
                    )
                })
                .max()
        })
    });
    let column_count = positive_integer(
        candidate
            .get("column_count")
            .or_else(|| candidate.get("columnCount"))
            .or_else(|| candidate.get("col_count")),
    )
    .or_else(|| {
        rows.as_ref().and_then(|rows| {
            rows.iter()
                .filter_map(|row| row.as_array().map(|row| row.len() as u64))
                .max()
        })
    })
    .or_else(|| {
        cells.as_ref().and_then(|cells| {
            cells
                .iter()
                .filter_map(|cell| {
                    Some(
                        cell["column_index"].as_u64()?
                            + cell.get("column_span").and_then(Value::as_u64).unwrap_or(1),
                    )
                })
                .max()
        })
    });
    let markdown = normalized_string(candidate.get("markdown"), maximum);
    let csv = normalized_string(candidate.get("csv"), maximum);
    let confidence = confidence(candidate.get("confidence"));
    if rows.is_none()
        && cells.is_none()
        && markdown.is_none()
        && csv.is_none()
        && row_count.is_none()
        && column_count.is_none()
        && confidence.is_none()
    {
        return None;
    }
    let mut output = Map::new();
    if let Some(value) = rows {
        output.insert("rows".into(), Value::Array(value));
    }
    if let Some(value) = markdown {
        output.insert("markdown".into(), json!(value));
    }
    if let Some(value) = csv {
        output.insert("csv".into(), json!(value));
    }
    if let Some(value) = row_count {
        output.insert("row_count".into(), json!(value));
    }
    if let Some(value) = column_count {
        output.insert("column_count".into(), json!(value));
    }
    if let Some(value) = cells {
        output.insert("cells".into(), Value::Array(value));
    }
    if let Some(value) = confidence {
        output.insert("confidence".into(), json!(value));
    }
    Some(Value::Object(output))
}
