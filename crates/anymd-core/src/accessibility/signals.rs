use serde_json::Value;

use super::{
    structure::{page_role_stats, RoleStats},
    AccessibilityInput,
};
use crate::fusion_util::round_ratio;

pub(super) struct PageSignals {
    pub(super) page: u32,
    pub(super) has_tree: bool,
    pub(super) stats: RoleStats,
    pub(super) visible: usize,
    pub(super) coverage: f64,
    pub(super) images: usize,
    pub(super) links: usize,
    pub(super) annotations: Vec<Value>,
    pub(super) fields: Vec<Value>,
}

impl PageSignals {
    pub(super) fn collect(input: &AccessibilityInput<'_>, trees: &[Value], page: u32) -> Self {
        let stats = page_role_stats(trees, page);
        let has_tree = stats.is_some();
        let stats = stats.unwrap_or_default();
        let page_elements = input
            .elements
            .as_array()
            .into_iter()
            .flatten()
            .filter(|element| element.get("page").and_then(Value::as_u64) == Some(u64::from(page)))
            .collect::<Vec<_>>();
        let visible = page_elements.len();
        let coverage = if !has_tree {
            0.0
        } else if visible == 0 {
            1.0
        } else {
            round_ratio((stats.contents as f64 / visible as f64).min(1.0))
        };
        let images = page_elements
            .iter()
            .filter(|element| element.get("type").and_then(Value::as_str) == Some("image"))
            .count();
        let annotations = page_annotations(input.annotations, page);
        let links = annotations
            .iter()
            .filter(|annotation| annotation.get("url").is_some())
            .count();
        let fields = page_fields(input.form_fields, page);
        Self {
            page,
            has_tree,
            stats,
            visible,
            coverage,
            images,
            links,
            annotations,
            fields,
        }
    }
}

fn page_annotations(value: Option<&Value>, page: u32) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|g| g.get("page").and_then(Value::as_u64) == Some(u64::from(page)))
        .and_then(|g| g.get("annotations"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn page_fields(value: Option<&Value>, page: u32) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| f.get("page").and_then(Value::as_u64) == Some(u64::from(page)))
        .cloned()
        .collect()
}
