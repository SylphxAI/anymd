//! Document AST input indexing, assembly, and response projection.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::super::{PageText, TRUST_REPORT_VERSION};
use super::aggregate::{ast_aggregate, DocumentAstNode};
use super::captions::{
    ast_link_captions_on_page, AstCaptionLinkBudget, AST_CAPTION_LINK_BUDGET_WARNING,
    MAX_AST_CAPTION_LINK_COMPARISONS,
};
use super::page::build_page_node;

pub(super) fn build_document_ast(
    pages: &[PageText],
    elements: &Value,
    chunks: &Value,
    warnings: &[String],
    visual_enrichments: &Value,
) -> Value {
    let mut selected_pages = pages.iter().map(|page| page.page).collect::<Vec<_>>();
    selected_pages.sort_unstable();
    selected_pages.dedup();

    let mut chunk_index = BTreeMap::<String, Vec<String>>::new();
    for chunk in chunks.as_array().into_iter().flatten() {
        let Some(chunk_id) = chunk.get("id").and_then(Value::as_str) else {
            continue;
        };
        for element_id in chunk
            .get("element_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            chunk_index
                .entry(element_id.to_string())
                .or_default()
                .push(chunk_id.to_string());
        }
    }

    let mut elements_by_page = BTreeMap::<u32, Vec<&Value>>::new();
    let mut range_start = u32::MAX;
    let mut range_end = 0;
    let mut has_heading = false;
    for element in elements.as_array().into_iter().flatten() {
        let Some(page) = element
            .get("page")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        range_start = range_start.min(page);
        range_end = range_end.max(page);
        has_heading |= element.get("type").and_then(Value::as_str) == Some("text")
            && element
                .pointer("/semantic_hint/role")
                .and_then(Value::as_str)
                == Some("heading");
        elements_by_page.entry(page).or_default().push(element);
    }
    if range_start == u32::MAX {
        range_start = 0;
    }

    let mut visual_by_target = BTreeMap::<String, &Value>::new();
    let mut visual_by_page = BTreeMap::<u32, Vec<&Value>>::new();
    for enrichment in visual_enrichments.as_array().into_iter().flatten() {
        if let Some(target) = enrichment.get("target_element_id").and_then(Value::as_str) {
            visual_by_target
                .entry(target.to_string())
                .or_insert(enrichment);
        }
        if let Some(page) = enrichment
            .get("page")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
        {
            visual_by_page.entry(page).or_default().push(enrichment);
        }
    }

    let mut document_sections = Vec::new();
    let mut page_nodes = Vec::new();
    let mut caption_link_budget = AstCaptionLinkBudget::new(MAX_AST_CAPTION_LINK_COMPARISONS);
    for page in &selected_pages {
        let mut page_node = build_page_node(
            *page,
            elements_by_page.remove(page).unwrap_or_default(),
            &chunk_index,
            &visual_by_target,
            visual_by_page
                .get(page)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            &mut document_sections,
        );
        ast_link_captions_on_page(&mut page_node, &mut caption_link_budget);
        page_nodes.push(page_node);
    }

    let mut root = DocumentAstNode {
        id: "document".into(),
        node_type: "document".into(),
        page_start: range_start,
        page_end: range_end,
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
        children: Some(page_nodes),
    };
    let stats = ast_aggregate(&mut root, 1);
    let visual_enrichment_kind_counts = Value::Object(
        stats
            .visual_enrichment_kind_counts
            .iter()
            .map(|(kind, count)| (kind.clone(), json!(count)))
            .collect(),
    );
    let mut output = json!({
        "version": TRUST_REPORT_VERSION,
        "profile": "document_ast",
        "root": root,
        "summary": {
            "selected_pages": selected_pages,
            "page_count": selected_pages.len(),
            "node_count": stats.node_count,
            "section_count": stats.section_count,
            "paragraph_count": stats.paragraph_count,
            "list_item_count": stats.list_item_count,
            "caption_count": stats.caption_count,
            "header_count": stats.header_count,
            "footer_count": stats.footer_count,
            "section_context_node_count": stats.section_context_node_count,
            "cross_page_section_context_count": stats.cross_page_section_context_count,
            "caption_link_count": stats.caption_link_count,
            "table_count": stats.table_count,
            "image_count": stats.image_count,
            "figure_count": stats.figure_count,
            "chart_count": stats.chart_count,
            "formula_count": stats.formula_count,
            "diagram_count": stats.diagram_count,
            "visual_enrichment_count": stats.visual_enrichment_count,
            "visual_enrichment_kind_counts": visual_enrichment_kind_counts,
            "max_depth": stats.max_depth,
        },
    });
    let mut ast_warnings = warnings.to_vec();
    if caption_link_budget.exhausted {
        ast_warnings.push(AST_CAPTION_LINK_BUDGET_WARNING.into());
    }
    if !has_heading {
        ast_warnings
            .push("No heading hierarchy detected; document_ast uses page-level leaf nodes.".into());
    }
    if !ast_warnings.is_empty() {
        output["warnings"] = json!(ast_warnings);
    }
    output
}
