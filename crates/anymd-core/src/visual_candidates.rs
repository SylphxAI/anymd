//! Provider-independent visual-enrichment candidate selection.
//!
//! This is a bounded functional core. It accepts the JSON document-element and
//! page-geometry projections, but performs no rendering or provider calls.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum document elements admitted by one selection request.
pub const MAX_VISUAL_CANDIDATE_SCAN_ELEMENTS: usize = 4_096;
/// Maximum page-geometry records admitted by one selection request.
pub const MAX_VISUAL_CANDIDATE_PAGE_GEOMETRY: usize = 4_096;
/// Maximum cross-element comparisons admitted by one selection request.
pub const MAX_VISUAL_CANDIDATE_COMPARISONS: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VisualBoundingBox {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualRegionRequest {
    pub id: String,
    pub page: u32,
    pub bounding_box: VisualBoundingBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualCandidateTargetType {
    Image,
    Table,
    Figure,
    Chart,
    Formula,
    Diagram,
}

impl VisualCandidateTargetType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Table => "table",
            Self::Figure => "figure",
            Self::Chart => "chart",
            Self::Formula => "formula",
            Self::Diagram => "diagram",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualEnrichmentCandidate {
    pub id: String,
    pub page: u32,
    pub region: VisualRegionRequest,
    pub target_element_id: String,
    pub target_element_type: VisualCandidateTargetType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_element_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_caption_element_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_caption_text: Option<String>,
    pub candidate_signals: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualCandidateLimit {
    Elements,
    PageGeometry,
    Comparisons,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualCandidateWarning {
    pub code: String,
    pub message: String,
    pub limit: VisualCandidateLimit,
    pub maximum: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualCandidateSelectionOutcome {
    pub candidates: Vec<VisualEnrichmentCandidate>,
    pub limited: bool,
    pub scanned_elements: usize,
    pub comparisons: usize,
    pub warnings: Vec<VisualCandidateWarning>,
}

impl VisualCandidateSelectionOutcome {
    fn limited(
        limit: VisualCandidateLimit,
        maximum: usize,
        scanned: usize,
        comparisons: usize,
    ) -> Self {
        Self {
            candidates: Vec::new(),
            limited: true,
            scanned_elements: scanned,
            comparisons,
            warnings: vec![VisualCandidateWarning {
                code: "visual_candidate_limit_exceeded".into(),
                message: format!(
                    "Visual candidate selection exceeded the request-wide {} limit of {maximum}.",
                    match limit {
                        VisualCandidateLimit::Elements => "element scan",
                        VisualCandidateLimit::PageGeometry => "page geometry",
                        VisualCandidateLimit::Comparisons => "comparison",
                    }
                ),
                limit,
                maximum,
            }],
        }
    }
}

#[derive(Debug, Clone)]
struct Element {
    id: String,
    kind: ElementKind,
    page: u32,
    bounding_box: Option<VisualBoundingBox>,
    content: Option<String>,
    semantic_role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Text,
    Image,
    Table,
}

impl Element {
    fn direct_kind(&self) -> Option<VisualCandidateTargetType> {
        match (self.kind, self.bounding_box) {
            (ElementKind::Image, Some(_)) => Some(VisualCandidateTargetType::Image),
            (ElementKind::Table, Some(_)) => Some(VisualCandidateTargetType::Table),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct AdmissionBudget {
    comparisons: usize,
}

impl AdmissionBudget {
    fn compare(&mut self) -> Result<(), ()> {
        if self.comparisons >= MAX_VISUAL_CANDIDATE_COMPARISONS {
            return Err(());
        }
        self.comparisons += 1;
        Ok(())
    }
}

/// Selects direct and caption-derived visual regions in stable element order.
///
/// Invalid elements and boxes are ignored. Exceeding any request-wide
/// admission cap fails closed with no partial candidates.
pub fn select_visual_enrichment_candidates(
    elements: &[Value],
    page_geometry: Option<&[Value]>,
    max_visual_enrichments: usize,
) -> VisualCandidateSelectionOutcome {
    if elements.len() > MAX_VISUAL_CANDIDATE_SCAN_ELEMENTS {
        return VisualCandidateSelectionOutcome::limited(
            VisualCandidateLimit::Elements,
            MAX_VISUAL_CANDIDATE_SCAN_ELEMENTS,
            MAX_VISUAL_CANDIDATE_SCAN_ELEMENTS,
            0,
        );
    }
    let page_geometry = page_geometry.unwrap_or_default();
    if page_geometry.len() > MAX_VISUAL_CANDIDATE_PAGE_GEOMETRY {
        return VisualCandidateSelectionOutcome::limited(
            VisualCandidateLimit::PageGeometry,
            MAX_VISUAL_CANDIDATE_PAGE_GEOMETRY,
            elements.len(),
            0,
        );
    }

    let parsed: Vec<_> = elements.iter().filter_map(parse_element).collect();
    let direct_targets: Vec<_> = parsed
        .iter()
        .filter(|element| element.direct_kind().is_some())
        .collect();
    let page_bounds = build_page_bounds_index(&parsed, page_geometry);
    let mut elements_by_page: HashMap<u32, Vec<&Element>> = HashMap::new();
    for element in &parsed {
        elements_by_page
            .entry(element.page)
            .or_default()
            .push(element);
    }

    let mut budget = AdmissionBudget::default();
    let mut candidates = Vec::new();
    for element in &parsed {
        if let (Some(kind), Some(bounding_box)) = (element.direct_kind(), element.bounding_box) {
            candidates.push(direct_candidate(element, kind, bounding_box));
            continue;
        }

        let Some(kind) = caption_kind(element) else {
            continue;
        };
        let Some(bounds) = page_bounds.get(&element.page).copied() else {
            continue;
        };
        match has_nearby_direct_target(element, kind, &direct_targets, &mut budget) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(()) => {
                return VisualCandidateSelectionOutcome::limited(
                    VisualCandidateLimit::Comparisons,
                    MAX_VISUAL_CANDIDATE_COMPARISONS,
                    elements.len(),
                    budget.comparisons,
                );
            }
        }

        let page_elements = elements_by_page
            .get(&element.page)
            .map(Vec::as_slice)
            .unwrap_or_default();
        match build_caption_candidate(element, kind, page_elements, bounds, &mut budget) {
            Ok(Some(candidate)) => candidates.push(candidate),
            Ok(None) => {}
            Err(()) => {
                return VisualCandidateSelectionOutcome::limited(
                    VisualCandidateLimit::Comparisons,
                    MAX_VISUAL_CANDIDATE_COMPARISONS,
                    elements.len(),
                    budget.comparisons,
                );
            }
        }
    }

    candidates.truncate(max_visual_enrichments.max(1));
    VisualCandidateSelectionOutcome {
        candidates,
        limited: false,
        scanned_elements: elements.len(),
        comparisons: budget.comparisons,
        warnings: Vec::new(),
    }
}

fn parse_element(value: &Value) -> Option<Element> {
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

fn parse_box(value: &Value) -> Option<VisualBoundingBox> {
    let box_ = VisualBoundingBox {
        left: value.get("left")?.as_f64()?,
        bottom: value.get("bottom")?.as_f64()?,
        right: value.get("right")?.as_f64()?,
        top: value.get("top")?.as_f64()?,
    };
    valid_box(box_).then_some(box_)
}

fn valid_box(box_: VisualBoundingBox) -> bool {
    [box_.left, box_.bottom, box_.right, box_.top]
        .iter()
        .all(|value| value.is_finite())
        && box_.right > box_.left
        && box_.top > box_.bottom
}

fn direct_candidate(
    element: &Element,
    kind: VisualCandidateTargetType,
    bounding_box: VisualBoundingBox,
) -> VisualEnrichmentCandidate {
    let signal = format!("{}-element", kind.as_str());
    let region = VisualRegionRequest {
        id: element.id.clone(),
        page: element.page,
        bounding_box,
    };
    VisualEnrichmentCandidate {
        id: element.id.clone(),
        page: element.page,
        region,
        target_element_id: element.id.clone(),
        target_element_type: kind,
        source_element_id: Some(element.id.clone()),
        source_caption_element_id: None,
        source_caption_text: None,
        candidate_signals: vec![signal, "element-bounding-box".into()],
    }
}

fn caption_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?iu)^(fig(?:ure)?|table|chart|graph|plot|formula|eq(?:uation)?|image|diagram|algorithm|exhibit)\.?(?:(?:\s*(?:\(?[a-z]?\d+(?:[.-]\d+)*[a-z]?\)?|\([A-Z]\)|[ivxlcdm]+)(?:\s*[:.)–—-]|\s+|$))|\s*[:)–—-])")
            .expect("static caption pattern")
    })
}

fn caption_kind(element: &Element) -> Option<VisualCandidateTargetType> {
    if element.kind != ElementKind::Text || element.bounding_box.is_none() {
        return None;
    }
    if matches!(
        element.semantic_role.as_deref(),
        Some("footer" | "header" | "heading" | "list_item")
    ) {
        return None;
    }
    let text = element.content.as_deref()?.trim();
    let raw = caption_pattern()
        .captures(text)?
        .get(1)?
        .as_str()
        .to_ascii_lowercase();
    match raw.as_str() {
        "table" => Some(VisualCandidateTargetType::Table),
        "formula" | "eq" | "equation" => Some(VisualCandidateTargetType::Formula),
        "chart" | "graph" | "plot" => Some(VisualCandidateTargetType::Chart),
        "image" => Some(VisualCandidateTargetType::Image),
        "diagram" => Some(VisualCandidateTargetType::Diagram),
        "algorithm" | "exhibit" | "fig" | "figure" => Some(VisualCandidateTargetType::Figure),
        _ => None,
    }
}

fn build_page_bounds_index(
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

fn union_two(left: VisualBoundingBox, right: VisualBoundingBox) -> VisualBoundingBox {
    VisualBoundingBox {
        left: left.left.min(right.left),
        bottom: left.bottom.min(right.bottom),
        right: left.right.max(right.right),
        top: left.top.max(right.top),
    }
}

fn horizontal_overlap_ratio(left: VisualBoundingBox, right: VisualBoundingBox) -> f64 {
    let overlap = left.right.min(right.right) - left.left.max(right.left);
    if overlap <= 0.0 {
        return 0.0;
    }
    let denominator = (left.right - left.left).min(right.right - right.left);
    if denominator > 0.0 {
        overlap / denominator
    } else {
        0.0
    }
}

fn vertical_overlap_ratio(left: VisualBoundingBox, right: VisualBoundingBox) -> f64 {
    let overlap = left.top.min(right.top) - left.bottom.max(right.bottom);
    if overlap <= 0.0 {
        return 0.0;
    }
    let denominator = (left.top - left.bottom).min(right.top - right.bottom);
    if denominator > 0.0 {
        overlap / denominator
    } else {
        0.0
    }
}

fn vertical_gap(left: VisualBoundingBox, right: VisualBoundingBox) -> f64 {
    if left.top < right.bottom {
        right.bottom - left.top
    } else if right.top < left.bottom {
        left.bottom - right.top
    } else {
        0.0
    }
}

fn horizontal_gap(left: VisualBoundingBox, right: VisualBoundingBox) -> f64 {
    if left.right < right.left {
        right.left - left.right
    } else if right.right < left.left {
        left.left - right.right
    } else {
        0.0
    }
}

fn direct_kind_matches(kind: VisualCandidateTargetType, target: &Element) -> bool {
    match kind {
        VisualCandidateTargetType::Table => target.kind == ElementKind::Table,
        VisualCandidateTargetType::Formula => false,
        _ => target.kind == ElementKind::Image,
    }
}

fn has_nearby_direct_target(
    caption: &Element,
    kind: VisualCandidateTargetType,
    targets: &[&Element],
    budget: &mut AdmissionBudget,
) -> Result<bool, ()> {
    let caption_box = caption.bounding_box.ok_or(())?;
    for target in targets {
        budget.compare()?;
        let target_box = target.bounding_box.ok_or(())?;
        if target.page == caption.page
            && direct_kind_matches(kind, target)
            && ((horizontal_overlap_ratio(caption_box, target_box) >= 0.12
                && vertical_gap(caption_box, target_box) <= 112.0)
                || (vertical_overlap_ratio(caption_box, target_box) >= 0.32
                    && horizontal_gap(caption_box, target_box) <= 112.0))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn caption_region_max_gap(kind: VisualCandidateTargetType, page: VisualBoundingBox) -> f64 {
    let height = page.top - page.bottom;
    match kind {
        VisualCandidateTargetType::Formula => (height * 0.16).clamp(84.0, 132.0),
        VisualCandidateTargetType::Table => (height * 0.24).clamp(128.0, 220.0),
        _ => (height * 0.32).clamp(168.0, 280.0),
    }
}

fn caption_region_max_side_gap(kind: VisualCandidateTargetType, page: VisualBoundingBox) -> f64 {
    let width = page.right - page.left;
    if kind == VisualCandidateTargetType::Formula {
        (width * 0.14).clamp(72.0, 112.0)
    } else {
        (width * 0.18).clamp(96.0, 160.0)
    }
}

fn visual_region_margin(kind: VisualCandidateTargetType, page: VisualBoundingBox) -> f64 {
    let width = page.right - page.left;
    if kind == VisualCandidateTargetType::Formula {
        (width * 0.025).clamp(12.0, 24.0)
    } else {
        (width * 0.035).clamp(16.0, 36.0)
    }
}

#[derive(Default)]
struct NeighborGroup {
    boxes: Vec<VisualBoundingBox>,
    minimum_gap: f64,
}

impl NeighborGroup {
    fn push(&mut self, box_: VisualBoundingBox, gap: f64) {
        if self.boxes.is_empty() || gap < self.minimum_gap {
            self.minimum_gap = gap;
        }
        self.boxes.push(box_);
    }
}

fn candidate_neighbor_boxes(
    caption: &Element,
    page_elements: &[&Element],
    page_bounds: VisualBoundingBox,
    kind: VisualCandidateTargetType,
    budget: &mut AdmissionBudget,
) -> Result<(Vec<VisualBoundingBox>, Vec<String>), ()> {
    let caption_box = caption.bounding_box.ok_or(())?;
    let max_gap = caption_region_max_gap(kind, page_bounds);
    let max_side_gap = caption_region_max_side_gap(kind, page_bounds);
    let mut above = NeighborGroup::default();
    let mut below = NeighborGroup::default();
    let mut left = NeighborGroup::default();
    let mut right = NeighborGroup::default();
    for element in page_elements {
        if element.id == caption.id
            || element.bounding_box.is_none()
            || (element.kind == ElementKind::Text
                && matches!(
                    element.semantic_role.as_deref(),
                    Some("caption" | "header" | "footer")
                ))
        {
            continue;
        }
        budget.compare()?;
        let box_ = element.bounding_box.ok_or(())?;
        if horizontal_overlap_ratio(caption_box, box_) >= 0.06 {
            if box_.bottom >= caption_box.top {
                let gap = box_.bottom - caption_box.top;
                if gap <= max_gap {
                    above.push(box_, gap);
                }
            } else if box_.top <= caption_box.bottom {
                let gap = caption_box.bottom - box_.top;
                if gap <= max_gap {
                    below.push(box_, gap);
                }
            } else if vertical_gap(caption_box, box_) == 0.0 {
                above.push(box_, 0.0);
            }
            continue;
        }
        if vertical_overlap_ratio(caption_box, box_) < 0.32 {
            continue;
        }
        if box_.right <= caption_box.left {
            let gap = caption_box.left - box_.right;
            if gap <= max_side_gap {
                left.push(box_, gap);
            }
        } else if box_.left >= caption_box.right {
            let gap = box_.left - caption_box.right;
            if gap <= max_side_gap {
                right.push(box_, gap);
            }
        }
    }

    let groups = [
        (above, "caption-target-above", 0.0),
        (below, "caption-target-below", 0.0),
        (left, "caption-target-left", 24.0),
        (right, "caption-target-right", 24.0),
    ];
    let selected = groups
        .into_iter()
        .filter(|(group, _, _)| !group.boxes.is_empty())
        .min_by(|left, right| {
            (left.0.minimum_gap + left.2).total_cmp(&(right.0.minimum_gap + right.2))
        });
    Ok(match selected {
        Some((group, signal, _)) => (
            group.boxes,
            vec!["nearby-positioned-evidence".into(), signal.into()],
        ),
        None => (Vec::new(), Vec::new()),
    })
}

fn fallback_caption_region(
    caption: VisualBoundingBox,
    page: VisualBoundingBox,
    kind: VisualCandidateTargetType,
) -> VisualBoundingBox {
    let page_width = page.right - page.left;
    let page_height = page.top - page.bottom;
    let caption_height = caption.top - caption.bottom;
    let center_x = (caption.left + caption.right) / 2.0;
    let center_y = (caption.bottom + caption.top) / 2.0;
    let vertical_span = if kind == VisualCandidateTargetType::Formula {
        (caption_height * 5.0).max(64.0).min(page_height * 0.22)
    } else {
        (page_height * 0.26).max(150.0).min(page_height * 0.42)
    };
    let half_width = if kind == VisualCandidateTargetType::Formula {
        (((caption.right - caption.left) / 2.0 + 48.0).max(120.0)).min(page_width / 2.0)
    } else {
        (page_width * 0.38).max(220.0).min(page_width / 2.0)
    };
    let left = page.left.max(center_x - half_width);
    let right = page.right.min(center_x + half_width);
    let prefer_above = caption.top + vertical_span <= page.top
        || center_y <= page.bottom + (page_height * 2.0) / 3.0;
    if prefer_above {
        VisualBoundingBox {
            left,
            bottom: caption.bottom,
            right,
            top: page.top.min(caption.top + vertical_span),
        }
    } else {
        VisualBoundingBox {
            left,
            bottom: page.bottom.max(caption.bottom - vertical_span),
            right,
            top: caption.top,
        }
    }
}

fn build_caption_candidate(
    caption: &Element,
    kind: VisualCandidateTargetType,
    page_elements: &[&Element],
    page_bounds: VisualBoundingBox,
    budget: &mut AdmissionBudget,
) -> Result<Option<VisualEnrichmentCandidate>, ()> {
    let caption_box = caption.bounding_box.ok_or(())?;
    let (neighbors, neighbor_signals) =
        candidate_neighbor_boxes(caption, page_elements, page_bounds, kind, budget)?;
    let source = if neighbors.is_empty() {
        fallback_caption_region(caption_box, page_bounds, kind)
    } else {
        neighbors.iter().copied().fold(caption_box, union_two)
    };
    let margin = visual_region_margin(kind, page_bounds);
    let box_ = VisualBoundingBox {
        left: page_bounds.left.max(source.left - margin),
        bottom: page_bounds.bottom.max(source.bottom - margin),
        right: page_bounds.right.min(source.right + margin),
        top: page_bounds.top.min(source.top + margin),
    };
    if !valid_box(box_) || box_.right - box_.left < 12.0 || box_.top - box_.bottom < 8.0 {
        return Ok(None);
    }
    let region_id = format!("{}-{}-region", caption.id, kind.as_str());
    let mut signals = vec![
        format!("caption-prefix-{}", kind.as_str()),
        "caption-bounding-box".into(),
    ];
    signals.extend(neighbor_signals);
    if neighbors.is_empty() {
        signals.push("caption-region-expansion".into());
    }
    let region = VisualRegionRequest {
        id: region_id.clone(),
        page: caption.page,
        bounding_box: box_,
    };
    Ok(Some(VisualEnrichmentCandidate {
        id: region_id.clone(),
        page: caption.page,
        region,
        target_element_id: region_id,
        target_element_type: kind,
        source_element_id: None,
        source_caption_element_id: Some(caption.id.clone()),
        source_caption_text: caption.content.as_deref().map(str::trim).map(str::to_owned),
        candidate_signals: signals,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn element(id: &str, kind: &str, page: u32, box_: Value) -> Value {
        json!({ "id": id, "type": kind, "page": page, "bounding_box": box_ })
    }

    fn caption(id: &str, content: &str, box_: Value) -> Value {
        json!({ "id": id, "type": "text", "page": 1, "content": content,
            "bounding_box": box_, "semantic_hint": { "role": "caption" } })
    }

    fn box_(left: f64, bottom: f64, right: f64, top: f64) -> Value {
        json!({ "left": left, "bottom": bottom, "right": right, "top": top })
    }

    fn geometry() -> Vec<Value> {
        vec![json!({ "page": 1, "width": 600, "height": 800, "rotation": 0 })]
    }

    #[test]
    fn direct_targets_preserve_order_and_apply_minimum_one_and_cap() {
        let elements = vec![
            element("image-1", "image", 1, box_(10.0, 10.0, 100.0, 100.0)),
            element("table-1", "table", 1, box_(10.0, 120.0, 100.0, 200.0)),
            element("image-2", "image", 1, box_(10.0, 220.0, 100.0, 300.0)),
        ];
        let one = select_visual_enrichment_candidates(&elements, None, 0);
        assert_eq!(
            one.candidates
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["image-1"]
        );
        let two = select_visual_enrichment_candidates(&elements, None, 2);
        assert_eq!(
            two.candidates
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["image-1", "table-1"]
        );
    }

    #[test]
    fn invalid_and_missing_boxes_fail_closed() {
        let elements = vec![
            json!({ "id": "missing", "type": "image", "page": 1 }),
            element("reversed", "table", 1, box_(20.0, 0.0, 10.0, 10.0)),
            element(
                "string",
                "image",
                1,
                json!({ "left": "NaN", "bottom": 0, "right": 10, "top": 10 }),
            ),
        ];
        assert!(select_visual_enrichment_candidates(&elements, None, 8)
            .candidates
            .is_empty());
    }

    #[test]
    fn caption_selects_above_before_side_and_falls_back_when_unaccompanied() {
        let bounds = box_(0.0, 0.0, 600.0, 800.0);
        let elements = vec![
            caption(
                "fig-1",
                "Figure 1: result",
                box_(250.0, 100.0, 350.0, 120.0),
            ),
            element("above", "text", 1, box_(200.0, 140.0, 400.0, 300.0)),
            element("side", "text", 1, box_(360.0, 100.0, 500.0, 120.0)),
            caption(
                "eq-1",
                "Equation (A) energy",
                box_(250.0, 600.0, 350.0, 620.0),
            ),
        ];
        let output = select_visual_enrichment_candidates(&elements, Some(&geometry()), 8);
        assert_eq!(output.candidates.len(), 2);
        assert!(output.candidates[0]
            .candidate_signals
            .contains(&"caption-target-above".into()));
        assert_eq!(
            output.candidates[0].region.bounding_box,
            VisualBoundingBox {
                left: 179.0,
                bottom: 79.0,
                right: 421.0,
                top: 321.0
            }
        );
        assert!(output.candidates[1]
            .candidate_signals
            .contains(&"caption-region-expansion".into()));
        assert!(output.candidates[1].region.bounding_box.top <= 800.0);
        assert!(parse_box(&bounds).is_some());
    }

    #[test]
    fn caption_uses_side_group_when_no_vertical_group_exists() {
        let elements = vec![
            caption(
                "chart-1",
                "Chart 2 - trend",
                box_(250.0, 300.0, 350.0, 330.0),
            ),
            element("left", "text", 1, box_(100.0, 300.0, 220.0, 330.0)),
        ];
        let output = select_visual_enrichment_candidates(&elements, Some(&geometry()), 8);
        assert!(output.candidates[0]
            .candidate_signals
            .contains(&"caption-target-left".into()));
    }

    #[test]
    fn matching_nearby_direct_target_suppresses_caption_region() {
        let elements = vec![
            caption(
                "table-caption",
                "Table 1: values",
                box_(100.0, 100.0, 300.0, 120.0),
            ),
            element("table", "table", 1, box_(100.0, 130.0, 300.0, 260.0)),
        ];
        let output = select_visual_enrichment_candidates(&elements, Some(&geometry()), 8);
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.candidates[0].id, "table");
    }

    #[test]
    fn element_scan_cap_admits_exact_and_rejects_cap_plus_one() {
        let exact = (0..MAX_VISUAL_CANDIDATE_SCAN_ELEMENTS)
            .map(|index| json!({ "id": format!("x-{index}"), "type": "text", "page": 1, "content": "plain" }))
            .collect::<Vec<_>>();
        assert!(!select_visual_enrichment_candidates(&exact, None, 8).limited);
        let mut over = exact;
        over.push(json!({ "id": "over", "type": "text", "page": 1, "content": "plain" }));
        let output = select_visual_enrichment_candidates(&over, None, 8);
        assert!(output.limited);
        assert_eq!(output.warnings[0].limit, VisualCandidateLimit::Elements);
    }

    #[test]
    fn page_geometry_cap_admits_exact_and_rejects_cap_plus_one() {
        let exact = (1..=MAX_VISUAL_CANDIDATE_PAGE_GEOMETRY)
            .map(|page| json!({ "page": page, "width": 600, "height": 800 }))
            .collect::<Vec<_>>();
        assert!(!select_visual_enrichment_candidates(&[], Some(&exact), 8).limited);
        let mut over = exact;
        over.push(
            json!({ "page": MAX_VISUAL_CANDIDATE_PAGE_GEOMETRY + 1, "width": 600, "height": 800 }),
        );
        let output = select_visual_enrichment_candidates(&[], Some(&over), 8);
        assert!(output.limited);
        assert_eq!(output.warnings[0].limit, VisualCandidateLimit::PageGeometry);
    }

    #[test]
    fn comparison_cap_admits_exact_and_rejects_next_comparison() {
        let mut exact = AdmissionBudget::default();
        for _ in 0..MAX_VISUAL_CANDIDATE_COMPARISONS {
            exact.compare().expect("exact cap admitted");
        }
        assert_eq!(exact.comparisons, MAX_VISUAL_CANDIDATE_COMPARISONS);
        assert!(exact.compare().is_err());

        let mut exact_request = vec![caption(
            "formula",
            "Formula 1: x",
            box_(10.0, 10.0, 80.0, 30.0),
        )];
        exact_request.extend((0..(MAX_VISUAL_CANDIDATE_COMPARISONS / 2)).map(|index| {
            element(
                &format!("exact-image-{index}"),
                "image",
                1,
                box_(200.0, 200.0, 210.0, 210.0),
            )
        }));
        let exact_output = select_visual_enrichment_candidates(
            &exact_request,
            Some(&geometry()),
            exact_request.len(),
        );
        assert!(!exact_output.limited);
        assert_eq!(exact_output.comparisons, MAX_VISUAL_CANDIDATE_COMPARISONS);

        let mut elements = vec![caption(
            "formula",
            "Formula 1: x",
            box_(10.0, 10.0, 80.0, 30.0),
        )];
        elements.extend((0..=MAX_VISUAL_CANDIDATE_COMPARISONS).map(|index| {
            element(
                &format!("image-{index}"),
                "image",
                1,
                box_(200.0, 200.0, 210.0, 210.0),
            )
        }));
        let output =
            select_visual_enrichment_candidates(&elements, Some(&geometry()), elements.len());
        assert!(output.limited);
        assert!(output.candidates.is_empty());
        assert_eq!(output.warnings[0].limit, VisualCandidateLimit::Comparisons);
    }
}
