//! Document-element and page-bounds projections used during candidate selection.

use std::collections::HashMap;

use serde_json::Value;

use super::geometry::{parse_box, union_two, valid_box};
use super::{VisualBoundingBox, VisualCandidateTargetType};

#[derive(Debug, Clone)]
pub(super) struct Element {
    pub(super) id: String,
    pub(super) kind: ElementKind,
    pub(super) page: u32,
    pub(super) bounding_box: Option<VisualBoundingBox>,
    pub(super) content: Option<String>,
    pub(super) semantic_role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ElementKind {
    Text,
    Image,
    Table,
}

impl Element {
    pub(super) fn direct_kind(&self) -> Option<VisualCandidateTargetType> {
        match (self.kind, self.bounding_box) {
            (ElementKind::Image, Some(_)) => Some(VisualCandidateTargetType::Image),
            (ElementKind::Table, Some(_)) => Some(VisualCandidateTargetType::Table),
            _ => None,
        }
    }
}

pub(super) fn parse_element(value: &Value) -> Option<Element> {
    let id = value.get("id")?.as_str()?.to_owned();
    let page = value
        .get("page")?
        .as_u64()
        .and_then(|page| u32::try_from(page).ok())?;
    let kind = match value.get("type")?.as_str()? {
        "text" => ElementKind::Text,
        "image" => ElementKind::Image,
        "table" => ElementKind::Table,
        _ => return None,
    };
    Some(Element {
        id,
        kind,
        page,
        bounding_box: value.get("bounding_box").and_then(parse_box),
        content: value
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned),
        semantic_role: value
            .pointer("/semantic_hint/role")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

pub(super) fn build_page_bounds_index(
    elements: &[Element],
    geometry: &[Value],
) -> HashMap<u32, VisualBoundingBox> {
    let mut bounds = HashMap::new();
    for item in geometry {
        let Some(page) = item
            .get("page")
            .and_then(Value::as_u64)
            .and_then(|page| u32::try_from(page).ok())
        else {
            continue;
        };
        let geometry_box = item.get("view_box").and_then(parse_box).or_else(|| {
            let width = item.get("width")?.as_f64()?;
            let height = item.get("height")?.as_f64()?;
            valid_box(VisualBoundingBox {
                left: 0.0,
                bottom: 0.0,
                right: width,
                top: height,
            })
            .then_some(VisualBoundingBox {
                left: 0.0,
                bottom: 0.0,
                right: width,
                top: height,
            })
        });
        if let Some(box_) = geometry_box {
            bounds.insert(page, box_);
        }
    }
    let mut fallback: HashMap<u32, VisualBoundingBox> = HashMap::new();
    for element in elements {
        if bounds.contains_key(&element.page) {
            continue;
        }
        if let Some(box_) = element.bounding_box {
            fallback
                .entry(element.page)
                .and_modify(|current| *current = union_two(*current, box_))
                .or_insert(box_);
        }
    }
    bounds.extend(fallback);
    bounds
}
