//! Page-local caption matching and AST link updates.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Value};

use super::super::DocumentAstNode;
use super::geometry::{ast_caption_geometry, ast_caption_link_box};
use super::AstCaptionLinkBudget;
use crate::document_twin::semantic::caption_prefix_pattern;
use crate::text_index::TextBoundingBox;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
struct AstCaptionLinkNode {
    id: String,
    node_type: String,
    text: Option<String>,
    element_id: String,
    bounding_box: Option<TextBoundingBox>,
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

pub(in super::super) fn ast_link_captions_on_page(
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
