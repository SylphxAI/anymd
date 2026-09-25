//! Document AST caption-to-target linking under a request-wide comparison budget.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Value};

use super::DocumentAstNode;
use crate::document_twin::semantic::caption_prefix_pattern;
use crate::text_index::TextBoundingBox;

pub(super) const MAX_AST_CAPTION_LINK_COMPARISONS: usize = 65_536;
pub(super) const AST_CAPTION_LINK_BUDGET_WARNING: &str =
    "Document AST caption linking stopped at the Rust request-wide comparison limit.";

#[derive(Debug, Clone)]
struct AstCaptionLinkNode {
    id: String,
    node_type: String,
    text: Option<String>,
    element_id: String,
    bounding_box: Option<TextBoundingBox>,
}

#[derive(Debug)]
pub(super) struct AstCaptionLinkBudget {
    remaining: usize,
    pub(super) exhausted: bool,
}

impl AstCaptionLinkBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            exhausted: false,
        }
    }

    fn admit_comparison(&mut self) -> bool {
        let Some(remaining) = self.remaining.checked_sub(1) else {
            self.exhausted = true;
            return false;
        };
        self.remaining = remaining;
        true
    }
}

fn ast_caption_link_box(node: &DocumentAstNode) -> Option<TextBoundingBox> {
    let box_ = node.bounding_boxes.first()?;
    let parsed = TextBoundingBox {
        left: box_.get("left")?.as_f64()?,
        bottom: box_.get("bottom")?.as_f64()?,
        right: box_.get("right")?.as_f64()?,
        top: box_.get("top")?.as_f64()?,
    };
    ([parsed.left, parsed.bottom, parsed.right, parsed.top]
        .into_iter()
        .all(f64::is_finite)
        && parsed.right > parsed.left
        && parsed.top > parsed.bottom)
        .then_some(parsed)
}

fn ast_collect_caption_link_nodes(node: &DocumentAstNode, output: &mut Vec<AstCaptionLinkNode>) {
    if node.node_type == "caption" || node.node_type == "table" {
        output.push(AstCaptionLinkNode {
            id: node.id.clone(),
            node_type: node.node_type.clone(),
            text: node.text.clone(),
            element_id: node
                .element_ids
                .first()
                .cloned()
                .unwrap_or_else(|| node.id.clone()),
            bounding_box: ast_caption_link_box(node),
        });
    }
    for child in node.children.as_deref().unwrap_or_default() {
        ast_collect_caption_link_nodes(child, output);
    }
}

fn ast_caption_kind(text: Option<&str>) -> Option<&'static str> {
    let raw = caption_prefix_pattern()
        .captures(text?.trim())?
        .get(1)?
        .as_str()
        .to_ascii_lowercase();
    match raw.as_str() {
        "table" => Some("table"),
        "chart" | "graph" | "plot" => Some("chart"),
        "formula" | "eq" | "equation" => Some("formula"),
        "image" => Some("image"),
        "diagram" => Some("diagram"),
        "algorithm" | "exhibit" | "fig" | "figure" => Some("figure"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct AstCaptionGeometry {
    relation: &'static str,
    gap: f64,
    max_gap: f64,
    overlap_ratio: f64,
    min_overlap_ratio: f64,
    overlap_signal: &'static str,
}

fn ast_overlap_ratio(left_start: f64, left_end: f64, right_start: f64, right_end: f64) -> f64 {
    let overlap = left_end.min(right_end) - left_start.max(right_start);
    let denominator = (left_end - left_start).min(right_end - right_start);
    if overlap <= 0.0 || denominator <= 0.0 {
        0.0
    } else {
        overlap / denominator
    }
}

fn ast_caption_geometry(caption: TextBoundingBox, target: TextBoundingBox) -> AstCaptionGeometry {
    if caption.top <= target.bottom {
        return AstCaptionGeometry {
            relation: "below",
            gap: target.bottom - caption.top,
            max_gap: 96.0,
            overlap_ratio: ast_overlap_ratio(
                caption.left,
                caption.right,
                target.left,
                target.right,
            ),
            min_overlap_ratio: 0.2,
            overlap_signal: "horizontal-overlap",
        };
    }
    if caption.bottom >= target.top {
        return AstCaptionGeometry {
            relation: "above",
            gap: caption.bottom - target.top,
            max_gap: 96.0,
            overlap_ratio: ast_overlap_ratio(
                caption.left,
                caption.right,
                target.left,
                target.right,
            ),
            min_overlap_ratio: 0.2,
            overlap_signal: "horizontal-overlap",
        };
    }
    if caption.right <= target.left {
        return AstCaptionGeometry {
            relation: "left",
            gap: target.left - caption.right,
            max_gap: 96.0,
            overlap_ratio: ast_overlap_ratio(
                caption.bottom,
                caption.top,
                target.bottom,
                target.top,
            ),
            min_overlap_ratio: 0.32,
            overlap_signal: "vertical-overlap",
        };
    }
    if caption.left >= target.right {
        return AstCaptionGeometry {
            relation: "right",
            gap: caption.left - target.right,
            max_gap: 96.0,
            overlap_ratio: ast_overlap_ratio(
                caption.bottom,
                caption.top,
                target.bottom,
                target.top,
            ),
            min_overlap_ratio: 0.32,
            overlap_signal: "vertical-overlap",
        };
    }
    let horizontal = ast_overlap_ratio(caption.left, caption.right, target.left, target.right);
    let vertical = ast_overlap_ratio(caption.bottom, caption.top, target.bottom, target.top);
    AstCaptionGeometry {
        relation: "overlapping",
        gap: 0.0,
        max_gap: 0.0,
        overlap_ratio: horizontal.max(vertical),
        min_overlap_ratio: 0.2,
        overlap_signal: if horizontal >= vertical {
            "horizontal-overlap"
        } else {
            "vertical-overlap"
        },
    }
}

fn ast_caption_link(
    caption: &AstCaptionLinkNode,
    target: &AstCaptionLinkNode,
    kind: Option<&str>,
) -> Option<(f64, Value)> {
    let geometry = ast_caption_geometry(caption.bounding_box?, target.bounding_box?);
    if geometry.overlap_ratio < geometry.min_overlap_ratio || geometry.gap > geometry.max_gap {
        return None;
    }
    let kind_matched = kind.is_some_and(|kind| kind == target.node_type);
    let confidence = (0.62 + geometry.overlap_ratio * 0.18 + if kind_matched { 0.12 } else { 0.0 }
        - geometry.gap / 480.0)
        .clamp(0.5, 0.95);
    let confidence = (confidence * 100.0).round() / 100.0;
    let mut signals = vec![
        "same-page".to_string(),
        geometry.overlap_signal.to_string(),
        format!("caption-{}", geometry.relation),
    ];
    if let Some(kind) = kind {
        signals.push(format!("caption-prefix-{kind}"));
    }
    if kind_matched {
        signals.push("caption-kind-match".into());
    }
    Some((
        confidence,
        json!({
            "node_id": target.id,
            "element_id": target.element_id,
            "type": target.node_type,
            "relation": geometry.relation,
            "confidence": confidence,
            "signals": signals,
        }),
    ))
}

fn ast_apply_caption_links(
    node: &mut DocumentAstNode,
    links: &BTreeMap<String, Value>,
    caption_ids: &BTreeMap<String, Vec<String>>,
) {
    if let Some(link) = links.get(&node.id) {
        node.caption_links = vec![link.clone()];
    }
    if let Some(ids) = caption_ids.get(&node.id) {
        node.caption_ids = ids.clone();
    }
    for child in node.children.as_deref_mut().unwrap_or_default() {
        ast_apply_caption_links(child, links, caption_ids);
    }
}

pub(super) fn ast_link_captions_on_page(
    page_node: &mut DocumentAstNode,
    budget: &mut AstCaptionLinkBudget,
) {
    let mut nodes = Vec::new();
    ast_collect_caption_link_nodes(page_node, &mut nodes);
    let captions = nodes
        .iter()
        .filter(|node| node.node_type == "caption")
        .collect::<Vec<_>>();
    let targets = nodes
        .iter()
        .filter(|node| node.node_type == "table")
        .collect::<Vec<_>>();
    let mut links = BTreeMap::<String, Value>::new();
    let mut caption_ids = BTreeMap::<String, Vec<String>>::new();
    let mut seen_caption_ids = BTreeMap::<String, HashSet<String>>::new();
    'captions: for caption in captions {
        let kind = ast_caption_kind(caption.text.as_deref());
        let mut best: Option<(f64, Value, String)> = None;
        for target in &targets {
            if !budget.admit_comparison() {
                break 'captions;
            }
            if kind.is_some_and(|kind| kind != target.node_type) {
                continue;
            }
            let Some((confidence, link)) = ast_caption_link(caption, target, kind) else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|(best_confidence, _, _)| confidence > *best_confidence)
            {
                best = Some((confidence, link, target.id.clone()));
            }
        }
        if let Some((_, link, target_id)) = best {
            links.insert(caption.id.clone(), link);
            if seen_caption_ids
                .entry(target_id.clone())
                .or_default()
                .insert(caption.id.clone())
            {
                caption_ids
                    .entry(target_id)
                    .or_default()
                    .push(caption.id.clone());
            }
        }
    }
    ast_apply_caption_links(page_node, &links, &caption_ids);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_twin::ast::nodes::ast_node_for_element;

    #[test]
    fn caption_link_geometry_matches_ts_boundaries_and_kind_filter() {
        let node = |id: &str, node_type: &str, text: Option<&str>, box_: TextBoundingBox| {
            AstCaptionLinkNode {
                id: id.into(),
                node_type: node_type.into(),
                text: text.map(str::to_string),
                element_id: id.into(),
                bounding_box: Some(box_),
            }
        };
        let caption = node(
            "caption",
            "caption",
            Some("Table 1: Exact boundary"),
            TextBoundingBox {
                left: 0.0,
                bottom: 0.0,
                right: 100.0,
                top: 10.0,
            },
        );
        let exact_vertical = node(
            "table",
            "table",
            None,
            TextBoundingBox {
                left: 80.0,
                bottom: 106.0,
                right: 180.0,
                top: 116.0,
            },
        );
        let (_, link) = ast_caption_link(&caption, &exact_vertical, Some("table")).unwrap();
        assert_eq!(link["relation"], "below");
        assert_eq!(link["confidence"], 0.58);

        let beyond_gap = AstCaptionLinkNode {
            bounding_box: Some(TextBoundingBox {
                bottom: 106.000_001,
                top: 116.000_001,
                ..exact_vertical.bounding_box.unwrap()
            }),
            ..exact_vertical.clone()
        };
        assert!(ast_caption_link(&caption, &beyond_gap, Some("table")).is_none());

        let side_caption = node(
            "side-caption",
            "caption",
            Some("Table 2: Side"),
            TextBoundingBox {
                left: 0.0,
                bottom: 0.0,
                right: 10.0,
                top: 100.0,
            },
        );
        let exact_side = node(
            "side-table",
            "table",
            None,
            TextBoundingBox {
                left: 106.0,
                bottom: 68.0,
                right: 116.0,
                top: 168.0,
            },
        );
        let (_, side_link) = ast_caption_link(&side_caption, &exact_side, Some("table")).unwrap();
        assert_eq!(side_link["relation"], "left");
        assert_eq!(side_link["confidence"], 0.6);

        let below_overlap = AstCaptionLinkNode {
            bounding_box: Some(TextBoundingBox {
                bottom: 68.000_001,
                top: 168.000_001,
                ..exact_side.bounding_box.unwrap()
            }),
            ..exact_side.clone()
        };
        assert!(ast_caption_link(&side_caption, &below_overlap, Some("table")).is_none());
        assert!(ast_caption_link(&caption, &exact_vertical, Some("figure")).is_some());
    }

    #[test]
    fn caption_link_budget_is_request_wide_and_exact() {
        let mut budget = AstCaptionLinkBudget::new(2);
        assert!(budget.admit_comparison());
        assert!(budget.admit_comparison());
        assert!(!budget.admit_comparison());
        assert!(budget.exhausted);
        assert_eq!(budget.remaining, 0);
    }

    #[test]
    fn caption_link_page_work_stops_before_max_plus_one_and_rejects_bad_boxes() {
        let empty_chunks = BTreeMap::new();
        let node = |value: Value| ast_node_for_element(&value, &empty_chunks, None).unwrap();
        let caption = |id: &str, box_: Value| {
            node(json!({
                "id":id,"type":"text","page":1,"content":"Table 1: Bounded",
                "bounding_box":box_,"semantic_hint":{"role":"caption"}
            }))
        };
        let table = node(json!({
            "id":"table","type":"table","page":1,
            "bounding_box":{"left":0,"bottom":30,"right":100,"top":80},
            "table":{"rows":[["A"]],"rowCount":1,"colCount":1,"confidence":1}
        }));
        let page = |children| DocumentAstNode {
            id: "p1".into(),
            node_type: "page".into(),
            page_start: 1,
            page_end: 1,
            element_ids: Vec::new(),
            visual_enrichment_ids: Vec::new(),
            chunk_ids: Vec::new(),
            bounding_boxes: Vec::new(),
            title: None,
            text: None,
            level: None,
            confidence: None,
            semantic_role: None,
            section_path: Vec::new(),
            continued_from_section_id: None,
            caption_links: Vec::new(),
            caption_ids: Vec::new(),
            table: None,
            image: None,
            formula: None,
            chart: None,
            visual_enrichment: None,
            children: Some(children),
        };

        let valid_box = json!({"left":0,"bottom":0,"right":100,"top":10});
        let mut bounded = page(vec![
            caption("caption-1", valid_box.clone()),
            caption("caption-2", valid_box),
            table.clone(),
        ]);
        let mut budget = AstCaptionLinkBudget::new(1);
        ast_link_captions_on_page(&mut bounded, &mut budget);
        let children = bounded.children.as_ref().unwrap();
        assert_eq!(children[0].caption_links.len(), 1);
        assert!(children[1].caption_links.is_empty());
        assert_eq!(children[2].caption_ids, vec!["caption-1"]);
        assert!(budget.exhausted);

        let mut malformed = page(vec![
            caption(
                "bad-caption",
                json!({"left":"bad","bottom":0,"right":100,"top":10}),
            ),
            table,
        ]);
        let mut malformed_budget = AstCaptionLinkBudget::new(2);
        ast_link_captions_on_page(&mut malformed, &mut malformed_budget);
        assert!(malformed.children.as_ref().unwrap()[0]
            .caption_links
            .is_empty());
        assert!(!malformed_budget.exhausted);
    }

    #[test]
    fn caption_link_exact_cap_keeps_reverse_id_dedup_linear() {
        let caption = |index: usize| DocumentAstNode {
            id: format!("caption-{index}"),
            node_type: "caption".into(),
            page_start: 1,
            page_end: 1,
            element_ids: vec![format!("caption-{index}")],
            visual_enrichment_ids: Vec::new(),
            chunk_ids: Vec::new(),
            bounding_boxes: vec![json!({"left":0,"bottom":0,"right":100,"top":10})],
            title: None,
            text: Some("Table: exact-cap hostile input".into()),
            level: None,
            confidence: None,
            semantic_role: Some("caption".into()),
            section_path: Vec::new(),
            continued_from_section_id: None,
            caption_links: Vec::new(),
            caption_ids: Vec::new(),
            table: None,
            image: None,
            formula: None,
            chart: None,
            visual_enrichment: None,
            children: None,
        };
        let mut children = (0..MAX_AST_CAPTION_LINK_COMPARISONS)
            .map(caption)
            .collect::<Vec<_>>();
        children.push(DocumentAstNode {
            id: "table".into(),
            node_type: "table".into(),
            page_start: 1,
            page_end: 1,
            element_ids: vec!["table".into()],
            visual_enrichment_ids: Vec::new(),
            chunk_ids: Vec::new(),
            bounding_boxes: vec![json!({"left":0,"bottom":30,"right":100,"top":80})],
            title: None,
            text: None,
            level: None,
            confidence: None,
            semantic_role: None,
            section_path: Vec::new(),
            continued_from_section_id: None,
            caption_links: Vec::new(),
            caption_ids: Vec::new(),
            table: Some(json!({})),
            image: None,
            formula: None,
            chart: None,
            visual_enrichment: None,
            children: None,
        });
        let mut page = DocumentAstNode {
            id: "p1".into(),
            node_type: "page".into(),
            page_start: 1,
            page_end: 1,
            element_ids: Vec::new(),
            visual_enrichment_ids: Vec::new(),
            chunk_ids: Vec::new(),
            bounding_boxes: Vec::new(),
            title: None,
            text: None,
            level: None,
            confidence: None,
            semantic_role: None,
            section_path: Vec::new(),
            continued_from_section_id: None,
            caption_links: Vec::new(),
            caption_ids: Vec::new(),
            table: None,
            image: None,
            formula: None,
            chart: None,
            visual_enrichment: None,
            children: Some(children),
        };
        let mut budget = AstCaptionLinkBudget::new(MAX_AST_CAPTION_LINK_COMPARISONS);

        ast_link_captions_on_page(&mut page, &mut budget);

        let children = page.children.as_ref().unwrap();
        let reverse_ids = &children.last().unwrap().caption_ids;
        assert_eq!(reverse_ids.len(), MAX_AST_CAPTION_LINK_COMPARISONS);
        assert_eq!(reverse_ids.first().unwrap(), "caption-0");
        assert_eq!(
            reverse_ids.last().unwrap(),
            &format!("caption-{}", MAX_AST_CAPTION_LINK_COMPARISONS - 1)
        );
        assert_eq!(budget.remaining, 0);
        assert!(!budget.exhausted);
    }
}
