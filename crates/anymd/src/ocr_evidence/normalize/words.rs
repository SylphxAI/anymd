use serde_json::{json, Value};

pub(super) fn normalize_confidence(value: &Value) -> Option<f64> {
    let value = value.as_f64()?;
    value.is_finite().then(|| {
        let value = if value > 1.0 { value / 100.0 } else { value };
        value.clamp(0.0, 1.0)
    })
}

fn round_coordinate(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(super) fn normalize_words(value: &Value, scale: f64) -> Option<Vec<Value>> {
    let words = value
        .as_array()?
        .iter()
        .filter_map(|word| {
            let word = word.as_object()?;
            let text = word.get("text")?.as_str()?;
            if text.trim().is_empty() {
                return None;
            }
            let mut output = json!({"text": text});
            for key in ["reading_order", "region_type", "layout_confidence"] {
                if let Some(value) = word.get(key) {
                    output[key] = value.clone();
                }
            }
            if let Some(confidence) = word.get("confidence").and_then(normalize_confidence) {
                output["confidence"] = json!(confidence);
            }
            if let Some(box_) = word.get("bounding_box").and_then(Value::as_object) {
                let left = box_.get("left").and_then(Value::as_f64);
                let bottom = box_.get("bottom").and_then(Value::as_f64);
                let right = box_.get("right").and_then(Value::as_f64);
                let top = box_.get("top").and_then(Value::as_f64);
                if let (Some(left), Some(bottom), Some(right), Some(top)) =
                    (left, bottom, right, top)
                {
                    if [left, bottom, right, top]
                        .iter()
                        .all(|value| value.is_finite())
                        && right > left
                        && top > bottom
                    {
                        output["bounding_box"] = json!({
                            "left": round_coordinate(left / scale),
                            "bottom": round_coordinate(bottom / scale),
                            "right": round_coordinate(right / scale),
                            "top": round_coordinate(top / scale),
                        });
                    }
                }
            }
            Some(output)
        })
        .collect::<Vec<_>>();
    (!words.is_empty()).then_some(words)
}
