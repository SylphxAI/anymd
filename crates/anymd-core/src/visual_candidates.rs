//! Provider-independent visual-enrichment candidate selection.
//!
//! This is a bounded functional core. It accepts the JSON document-element and
//! page-geometry projections, but performs no rendering or provider calls.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod caption;
mod geometry;
mod projection;

use caption::{build_caption_candidate, caption_kind, has_nearby_direct_target};
use projection::{build_page_bounds_index, parse_element, Element};

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

#[cfg(test)]
mod tests;
