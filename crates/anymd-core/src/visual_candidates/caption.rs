//! Caption classification, direct-target suppression, and candidate regions.

use std::sync::OnceLock;

use regex::Regex;

use super::geometry::{
    horizontal_gap, horizontal_overlap_ratio, union_two, valid_box, vertical_gap,
    vertical_overlap_ratio,
};
use super::projection::{Element, ElementKind};
use super::{
    AdmissionBudget, VisualBoundingBox, VisualCandidateTargetType, VisualEnrichmentCandidate,
    VisualRegionRequest,
};

fn caption_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?iu)^(fig(?:ure)?|table|chart|graph|plot|formula|eq(?:uation)?|image|diagram|algorithm|exhibit)\.?(?:(?:\s*(?:\(?[a-z]?\d+(?:[.-]\d+)*[a-z]?\)?|\([A-Z]\)|[ivxlcdm]+)(?:\s*[:.)–—-]|\s+|$))|\s*[:)–—-])")
            .expect("static caption pattern")
    })
}

pub(super) fn caption_kind(element: &Element) -> Option<VisualCandidateTargetType> {
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

fn direct_kind_matches(kind: VisualCandidateTargetType, target: &Element) -> bool {
    match kind {
        VisualCandidateTargetType::Table => target.kind == ElementKind::Table,
        VisualCandidateTargetType::Formula => false,
        _ => target.kind == ElementKind::Image,
    }
}

pub(super) fn has_nearby_direct_target(
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

pub(super) fn build_caption_candidate(
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
