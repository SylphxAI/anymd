//! Page subtree construction and cross-page section context.

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use super::aggregate::{
    ast_children_at_path, ast_section_ref, DocumentAstNode, DocumentAstSectionRef,
};
use super::nodes::{ast_node_for_element, node_for_visual_enrichment};

pub(super) fn build_page_node(
    page: u32,
    page_elements: Vec<&Value>,
    chunk_index: &BTreeMap<String, Vec<String>>,
    visual_by_target: &BTreeMap<String, &Value>,
    page_enrichments: &[&Value],
    document_sections: &mut Vec<DocumentAstSectionRef>,
) -> DocumentAstNode {
    let mut page_children = Vec::<DocumentAstNode>::new();
    let mut page_section_stack = Vec::<(u64, Vec<usize>)>::new();
    let page_element_ids = page_elements
        .iter()
        .filter_map(|element| element.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<HashSet<_>>();
    for element in page_elements {
        let element_id = element.get("id").and_then(Value::as_str);
        let enrichment = element_id.and_then(|id| visual_by_target.get(id).copied());
        let Some(mut node) = ast_node_for_element(element, chunk_index, enrichment) else {
            continue;
        };

        if node.node_type != "header" && node.node_type != "footer" {
            if node.node_type == "section" {
                let level = node.level.unwrap_or(1);
                while document_sections
                    .last()
                    .is_some_and(|section| section.level >= level)
                {
                    document_sections.pop();
                }
                node.section_path = document_sections.clone();
                node.section_path.push(ast_section_ref(&node));
                node.continued_from_section_id = node
                    .section_path
                    .iter()
                    .rev()
                    .find(|section| section.page_start < node.page_start)
                    .map(|section| section.id.clone());
                document_sections.push(ast_section_ref(&node));
            } else if !document_sections.is_empty() {
                node.section_path = document_sections.clone();
                node.continued_from_section_id = node
                    .section_path
                    .iter()
                    .rev()
                    .find(|section| section.page_start < node.page_start)
                    .map(|section| section.id.clone());
            }
        }

        if node.node_type == "header" || node.node_type == "footer" {
            page_children.push(node);
        } else if node.node_type == "section" {
            let level = node.level.unwrap_or(1);
            while page_section_stack
                .last()
                .is_some_and(|(parent_level, _)| *parent_level >= level)
            {
                page_section_stack.pop();
            }
            let parent_path = page_section_stack
                .last()
                .map(|(_, path)| path.clone())
                .unwrap_or_default();
            let parent_children = ast_children_at_path(&mut page_children, &parent_path);
            let index = parent_children.len();
            parent_children.push(node);
            let mut path = parent_path;
            path.push(index);
            page_section_stack.push((level, path));
        } else {
            let parent_path = page_section_stack
                .last()
                .map(|(_, path)| path.as_slice())
                .unwrap_or(&[]);
            ast_children_at_path(&mut page_children, parent_path).push(node);
        }
    }

    for enrichment in page_enrichments {
        let target = enrichment
            .get("target_element_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        if page_element_ids.contains(target) {
            continue;
        }
        let Some(mut node) = node_for_visual_enrichment(enrichment) else {
            continue;
        };
        if !document_sections.is_empty() {
            node.section_path = document_sections.clone();
            node.continued_from_section_id = node
                .section_path
                .iter()
                .rev()
                .find(|section| section.page_start < node.page_start)
                .map(|section| section.id.clone());
        }
        let parent_path = page_section_stack
            .last()
            .map(|(_, path)| path.as_slice())
            .unwrap_or(&[]);
        ast_children_at_path(&mut page_children, parent_path).push(node);
    }

    DocumentAstNode {
        id: format!("p{page}"),
        node_type: "page".into(),
        page_start: page,
        page_end: page,
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
        children: Some(page_children),
    }
}
