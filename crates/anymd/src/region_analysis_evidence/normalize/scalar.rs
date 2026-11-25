//! Scalar field normalization shared by all region shapes.

use serde_json::Value;

use crate::pdf_evidence_common::truncate_utf16;

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
