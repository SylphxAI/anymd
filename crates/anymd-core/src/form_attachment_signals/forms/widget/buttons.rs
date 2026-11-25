use lopdf::Dictionary;
use serde_json::Value;

use super::super::super::walker::Walker;
use super::form_value;

pub(super) fn normalize_button<'a>(
    walker: &mut Walker<'a>,
    dict: &'a Dictionary,
    field_type: Option<&str>,
    value: &mut Option<Value>,
    default_value: &mut Option<Value>,
) {
    // pdf.js _processCheckBox: when AP/N is a named-state dict, AS string
    // overwrites fieldValue, then exportValues normalization may force Off.
    if field_type == Some("checkbox") && button_has_named_normal_appearance(walker, dict) {
        if let Some(as_value) = dict.get(b"AS").ok().and_then(|v| form_value(walker, v)) {
            match &as_value {
                Value::String(s) if !s.is_empty() => *value = Some(as_value),
                _ => {}
            }
        }
        if let Some(export_values) = checkbox_named_export_values(walker, dict, value.as_ref()) {
            let current = match &*value {
                Some(Value::String(s)) => Some(s.as_str()),
                _ => None,
            };
            if current.is_none_or(|s| !export_values.iter().any(|k| k == s)) {
                *value = Some(Value::String("Off".into()));
            }
        }
    }
    // pdf.js getFieldObjects keeps non-empty button V/DV arrays as string
    // arrays; only missing/null/empty/non-string non-array values collapse
    // to the public "Off" sentinel.
    *value = Some(match value.take() {
        Some(Value::Array(values)) if !values.is_empty() => Value::Array(values),
        Some(Value::String(value)) if !value.is_empty() => Value::String(value),
        _ => Value::String("Off".into()),
    });
    // pdf.js _processCheckBox/_processRadioButton only (not pushbutton):
    // when AP/N is a named-state dict and DV is null, defaultFieldValue
    // becomes "Off". Pushbutton and non-named AP leave default null.
    if default_value.as_ref().is_none_or(Value::is_null) {
        let is_checkbox_or_radio = matches!(field_type, Some("checkbox") | Some("radiobutton"));
        if is_checkbox_or_radio && button_has_named_normal_appearance(walker, dict) {
            *default_value = Some(Value::String("Off".into()));
        } else {
            default_value.get_or_insert(Value::Null);
        }
    }
}

fn button_has_named_normal_appearance<'a>(walker: &mut Walker<'a>, dict: &'a Dictionary) -> bool {
    // pdf.js ButtonWidgetAnnotation _processCheckBox/_processRadioButton require
    // AP dict and AP/N named-state dict before setting defaultFieldValue = "Off".
    let Ok(ap) = dict.get(b"AP") else {
        return false;
    };
    let Some(ap_dict) = walker.resolve(ap).and_then(|value| value.as_dict().ok()) else {
        return false;
    };
    let Ok(normal) = ap_dict.get(b"N") else {
        return false;
    };
    walker
        .resolve(normal)
        .and_then(|value| value.as_dict().ok())
        .is_some()
}

fn checkbox_named_export_values<'a>(
    walker: &mut Walker<'a>,
    dict: &'a Dictionary,
    field_value: Option<&Value>,
) -> Option<Vec<String>> {
    // pdf.js _processCheckBox exportValues construction from AP/N keys.
    let Ok(ap) = dict.get(b"AP") else {
        return None;
    };
    let ap_dict = walker.resolve(ap).and_then(|value| value.as_dict().ok())?;
    let Ok(normal) = ap_dict.get(b"N") else {
        return None;
    };
    let normal_dict = walker
        .resolve(normal)
        .and_then(|value| value.as_dict().ok())?;
    let mut keys: Vec<String> = normal_dict
        .iter()
        .filter_map(|(k, _)| std::str::from_utf8(k).ok().map(str::to_string))
        .collect();
    let yes = match field_value {
        Some(Value::String(s)) if !s.is_empty() && s != "Off" => s.clone(),
        _ => "Yes".to_string(),
    };
    if keys.is_empty() {
        keys.push("Off".into());
        keys.push(yes);
    } else if keys.len() == 1 {
        if keys[0] == "Off" {
            keys.push(yes);
        } else {
            keys.insert(0, "Off".into());
        }
    } else if keys.iter().any(|k| k == &yes) {
        keys.clear();
        keys.push("Off".into());
        keys.push(yes);
    } else {
        let other_yes = keys
            .iter()
            .find(|k| k.as_str() != "Off")
            .cloned()
            .unwrap_or_else(|| "Yes".into());
        keys.clear();
        keys.push("Off".into());
        keys.push(other_yes);
    }
    Some(keys)
}
