//! Document AST nodes, tree navigation, and recursive aggregation.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub(super) struct DocumentAstSectionRef {
    pub(super) id: String,
    title: String,
    pub(super) level: u64,
    pub(super) page_start: u32,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DocumentAstNode {
    pub(super) id: String,
    #[serde(rename = "type")]
    pub(super) node_type: String,
    pub(super) page_start: u32,
    pub(super) page_end: u32,
    pub(super) element_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) chunk_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) visual_enrichment_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) bounding_boxes: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) level: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) confidence: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) semantic_role: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) section_path: Vec<DocumentAstSectionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) continued_from_section_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) caption_links: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) caption_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) table: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) image: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) formula: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) chart: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) visual_enrichment: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) children: Option<Vec<DocumentAstNode>>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DocumentAstStats {
    pub(super) node_count: usize,
    pub(super) section_count: usize,
    pub(super) paragraph_count: usize,
    pub(super) list_item_count: usize,
    pub(super) caption_count: usize,
    pub(super) header_count: usize,
    pub(super) footer_count: usize,
    pub(super) section_context_node_count: usize,
    pub(super) cross_page_section_context_count: usize,
    pub(super) caption_link_count: usize,
    pub(super) table_count: usize,
    pub(super) image_count: usize,
    pub(super) figure_count: usize,
    pub(super) chart_count: usize,
    pub(super) formula_count: usize,
    pub(super) diagram_count: usize,
    pub(super) visual_enrichment_count: usize,
    pub(super) visual_enrichment_kind_counts: BTreeMap<String, usize>,
    pub(super) max_depth: usize,
}

fn ast_unique_extend(target: &mut Vec<String>, values: impl IntoIterator<Item = String>) {
    let mut seen = target.iter().cloned().collect::<HashSet<_>>();
    for value in values {
        if seen.insert(value.clone()) {
            target.push(value);
        }
    }
}

fn ast_unique_boxes(target: &mut Vec<Value>, values: impl IntoIterator<Item = Value>) {
    let key = |value: &Value| {
        ["left", "bottom", "right", "top"]
            .map(|field| value.get(field).unwrap_or(&Value::Null).to_string())
            .join(":")
    };
    let mut seen = target.iter().map(key).collect::<HashSet<_>>();
    for value in values {
        if seen.insert(key(&value)) {
            target.push(value);
        }
    }
}

pub(super) fn ast_children_at_path<'a>(
    children: &'a mut Vec<DocumentAstNode>,
    path: &[usize],
) -> &'a mut Vec<DocumentAstNode> {
    let Some((&index, tail)) = path.split_first() else {
        return children;
    };
    ast_children_at_path(children[index].children.get_or_insert_with(Vec::new), tail)
}

pub(super) fn ast_section_ref(node: &DocumentAstNode) -> DocumentAstSectionRef {
    DocumentAstSectionRef {
        id: node.id.clone(),
        title: node
            .title
            .clone()
            .or_else(|| node.text.clone())
            .unwrap_or_else(|| node.id.clone()),
        level: node.level.unwrap_or(1),
        page_start: node.page_start,
    }
}

pub(super) fn ast_aggregate(node: &mut DocumentAstNode, depth: usize) -> DocumentAstStats {
    let mut stats = DocumentAstStats {
        node_count: 1,
        section_count: usize::from(node.node_type == "section"),
        paragraph_count: usize::from(node.node_type == "paragraph"),
        list_item_count: usize::from(node.node_type == "list_item"),
        caption_count: usize::from(node.node_type == "caption"),
        header_count: usize::from(node.node_type == "header"),
        footer_count: usize::from(node.node_type == "footer"),
        section_context_node_count: usize::from(!node.section_path.is_empty()),
        cross_page_section_context_count: usize::from(node.continued_from_section_id.is_some()),
        caption_link_count: node.caption_links.len(),
        table_count: usize::from(node.node_type == "table"),
        image_count: usize::from(node.image.is_some() || node.node_type == "image"),
        figure_count: usize::from(node.node_type == "figure"),
        chart_count: usize::from(node.node_type == "chart"),
        formula_count: usize::from(node.node_type == "formula"),
        diagram_count: usize::from(node.node_type == "diagram"),
        visual_enrichment_count: usize::from(node.visual_enrichment.is_some()),
        visual_enrichment_kind_counts: node
            .visual_enrichment
            .as_ref()
            .and_then(|value| value.get("kind"))
            .and_then(Value::as_str)
            .map(|kind| BTreeMap::from([(kind.to_string(), 1usize)]))
            .unwrap_or_default(),
        max_depth: depth,
    };

    let mut child_element_ids = Vec::new();
    let mut child_chunk_ids = Vec::new();
    let mut child_visual_enrichment_ids = Vec::new();
    let mut child_boxes = Vec::new();
    if let Some(children) = node.children.as_mut() {
        for child in children.iter_mut() {
            let child_stats = ast_aggregate(child, depth + 1);
            child_element_ids.extend(child.element_ids.clone());
            child_chunk_ids.extend(child.chunk_ids.clone());
            child_visual_enrichment_ids.extend(child.visual_enrichment_ids.clone());
            child_boxes.extend(child.bounding_boxes.clone());
            stats.node_count += child_stats.node_count;
            stats.section_count += child_stats.section_count;
            stats.paragraph_count += child_stats.paragraph_count;
            stats.list_item_count += child_stats.list_item_count;
            stats.caption_count += child_stats.caption_count;
            stats.header_count += child_stats.header_count;
            stats.footer_count += child_stats.footer_count;
            stats.section_context_node_count += child_stats.section_context_node_count;
            stats.cross_page_section_context_count += child_stats.cross_page_section_context_count;
            stats.caption_link_count += child_stats.caption_link_count;
            stats.table_count += child_stats.table_count;
            stats.image_count += child_stats.image_count;
            stats.figure_count += child_stats.figure_count;
            stats.chart_count += child_stats.chart_count;
            stats.formula_count += child_stats.formula_count;
            stats.diagram_count += child_stats.diagram_count;
            stats.visual_enrichment_count += child_stats.visual_enrichment_count;
            for (kind, count) in child_stats.visual_enrichment_kind_counts {
                *stats.visual_enrichment_kind_counts.entry(kind).or_default() += count;
            }
            stats.max_depth = stats.max_depth.max(child_stats.max_depth);
        }
        if let Some(first) = children.first() {
            node.page_start = node.page_start.min(first.page_start);
            node.page_end = node.page_end.max(first.page_end);
        }
        for child in children.iter().skip(1) {
            node.page_start = node.page_start.min(child.page_start);
            node.page_end = node.page_end.max(child.page_end);
        }
    }
    ast_unique_extend(&mut node.element_ids, child_element_ids);
    ast_unique_extend(&mut node.chunk_ids, child_chunk_ids);
    ast_unique_extend(&mut node.visual_enrichment_ids, child_visual_enrichment_ids);
    ast_unique_boxes(&mut node.bounding_boxes, child_boxes);
    stats
}
