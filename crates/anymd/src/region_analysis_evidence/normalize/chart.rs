//! Chart axes, data points and series.

use serde_json::{json, Map, Value};

use super::scalar::{confidence, normalized_string};

fn data_points(value: Option<&Value>) -> Option<Value> {
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

fn chart_axis(value: Option<&Value>, maximum: usize) -> Option<Value> {
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

fn chart_series(value: Option<&Value>, maximum: usize) -> Option<Value> {
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
