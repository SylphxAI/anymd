//! Normalization of region-analysis provider output into the public kind, table, formula and chart shapes.

use super::*;

pub(super) fn truncate_utf16(text: &str, maximum: usize) -> (String, bool) {
    let mut units = 0usize;
    let mut end = text.len();
    for (index, character) in text.char_indices() {
        let next = units + character.len_utf16();
        if next > maximum {
            end = index;
            break;
        }
        units = next;
    }
    (
        text[..end].to_string(),
        text.encode_utf16().count() > maximum,
    )
}

pub(super) fn normalized_string(value: Option<&Value>, maximum: usize) -> Option<String> {
    let trimmed = value?.as_str()?.trim();
    (!trimmed.is_empty()).then(|| truncate_utf16(trimmed, maximum).0)
}

pub(super) fn confidence(value: Option<&Value>) -> Option<f64> {
    let value = value?.as_f64()?;
    value.is_finite().then(|| {
        let normalized = if value > 1.0 { value / 100.0 } else { value };
        normalized.clamp(0.0, 1.0)
    })
}

pub(super) fn positive_integer(value: Option<&Value>) -> Option<u64> {
    value?.as_u64().filter(|value| *value > 0)
}

pub(super) fn zero_integer(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

pub(super) fn bounding_box(value: Option<&Value>) -> Option<Value> {
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

pub(super) fn rows(value: Option<&Value>) -> Option<Vec<Value>> {
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

pub(super) fn table_cells(value: Option<&Value>, maximum: usize) -> Option<Vec<Value>> {
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

pub(super) fn normalize_formula(value: Option<&Value>, maximum: usize) -> Option<Value> {
    let candidate = value?.as_object()?;
    let mut output = Map::new();
    for (source, target) in [
        ("latex", "latex"),
        ("mathml", "mathml"),
        ("asciimath", "asciimath"),
        ("text", "text"),
    ] {
        if let Some(value) = normalized_string(candidate.get(source), maximum) {
            output.insert(target.into(), json!(value));
        }
    }
    if !output.contains_key("asciimath") {
        if let Some(value) = normalized_string(candidate.get("ascii_math"), maximum) {
            output.insert("asciimath".into(), json!(value));
        }
    }
    if let Some(value) = confidence(candidate.get("confidence")) {
        output.insert("confidence".into(), json!(value));
    }
    (!output.is_empty()).then_some(Value::Object(output))
}

pub(super) fn data_points(value: Option<&Value>) -> Option<Value> {
    let points = value?
        .as_array()?
        .iter()
        .filter_map(|point| {
            let point = point.as_object()?;
            let output = point
                .iter()
                .filter(|(_, value)| {
                    matches!(
                        value,
                        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
                    )
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>();
            (!output.is_empty()).then_some(Value::Object(output))
        })
        .collect::<Vec<_>>();
    (!points.is_empty()).then_some(Value::Array(points))
}

pub(super) fn chart_axis(value: Option<&Value>, maximum: usize) -> Option<Value> {
    let candidate = value?.as_object()?;
    let mut output = Map::new();
    for key in ["label", "unit"] {
        if let Some(value) = normalized_string(candidate.get(key), maximum) {
            output.insert(key.into(), json!(value));
        }
    }
    for key in ["min", "max"] {
        if candidate
            .get(key)
            .and_then(Value::as_f64)
            .is_some_and(f64::is_finite)
        {
            output.insert(key.into(), candidate[key].clone());
        }
    }
    (!output.is_empty()).then_some(Value::Object(output))
}

pub(super) fn chart_series(value: Option<&Value>, maximum: usize) -> Option<Value> {
    let series = value?
        .as_array()?
        .iter()
        .filter_map(|entry| {
            let entry = entry.as_object()?;
            let points = data_points(entry.get("data_points").or_else(|| entry.get("points")))?;
            let mut output = Map::new();
            if let Some(value) = normalized_string(entry.get("name"), maximum) {
                output.insert("name".into(), json!(value));
            }
            output.insert("data_points".into(), points);
            if let Some(value) = confidence(entry.get("confidence")) {
                output.insert("confidence".into(), json!(value));
            }
            Some(Value::Object(output))
        })
        .collect::<Vec<_>>();
    (!series.is_empty()).then_some(Value::Array(series))
}

pub(super) fn normalize_chart(value: Option<&Value>, maximum: usize) -> Option<Value> {
    let candidate = value?.as_object()?;
    let mut output = Map::new();
    for key in ["title", "summary"] {
        if let Some(value) = normalized_string(candidate.get(key), maximum) {
            output.insert(key.into(), json!(value));
        }
    }
    if let Some(value) = data_points(candidate.get("data_points")) {
        output.insert("data_points".into(), value);
    }
    if let Some(value) = chart_axis(candidate.get("x_axis"), maximum) {
        output.insert("x_axis".into(), value);
    }
    if let Some(value) = chart_axis(candidate.get("y_axis"), maximum) {
        output.insert("y_axis".into(), value);
    }
    if let Some(value) = chart_series(candidate.get("series"), maximum) {
        output.insert("series".into(), value);
    }
    if let Some(value) = confidence(candidate.get("confidence")) {
        output.insert("confidence".into(), json!(value));
    }
    (!output.is_empty()).then_some(Value::Object(output))
}

pub(super) fn normalize_output(stdout: &str, maximum: usize) -> Value {
    let trimmed = stdout.trim();
    let parsed = serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object);
    let Some(parsed) = parsed else {
        let (description, truncated) = truncate_utf16(trimmed, maximum);
        let mut output = json!({"kind": "unknown", "description": description});
        if truncated {
            output["warnings"] = json!([format!(
                "Region analysis output truncated to {maximum} characters."
            )]);
        }
        return output;
    };
    let object = parsed.as_object().expect("checked object");
    let mut warnings = object
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|warning| {
            warning
                .as_str()
                .map(str::trim)
                .filter(|warning| !warning.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "unknown".into());
    let kind = if matches!(
        kind.as_str(),
        "text" | "table" | "figure" | "chart" | "formula" | "image" | "diagram" | "unknown"
    ) {
        kind
    } else {
        warnings.push(format!(
            "Unsupported region analysis kind \"{kind}\"; normalized to \"unknown\"."
        ));
        "unknown".into()
    };
    let mut output = Map::from_iter([("kind".into(), json!(kind))]);
    for key in ["description", "text", "markdown"] {
        if let Some(value) = normalized_string(object.get(key), maximum) {
            output.insert(key.into(), json!(value));
        }
    }
    if let Some(value) = confidence(object.get("confidence")) {
        output.insert("confidence".into(), json!(value));
    }
    if let Some(value) = normalize_table(object.get("table"), maximum) {
        output.insert("table".into(), value);
    }
    if let Some(value) = normalize_formula(object.get("formula"), maximum) {
        output.insert("formula".into(), value);
    }
    if let Some(value) = normalize_chart(object.get("chart"), maximum) {
        output.insert("chart".into(), value);
    }
    if !warnings.is_empty() {
        output.insert("warnings".into(), json!(warnings));
    }
    Value::Object(output)
}
