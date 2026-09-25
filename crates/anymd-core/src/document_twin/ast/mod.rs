//! Document AST: hierarchy, section context, aggregation, and visual enrichment attachment.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use serde_json::{json, Value};

use self::captions::{
    ast_link_captions_on_page, AstCaptionLinkBudget, AST_CAPTION_LINK_BUDGET_WARNING,
    MAX_AST_CAPTION_LINK_COMPARISONS,
};
use self::nodes::{ast_node_for_element, node_for_visual_enrichment};
use super::{PageText, TRUST_REPORT_VERSION};

mod captions;
mod nodes;

#[derive(Debug, Clone, Serialize)]
struct DocumentAstSectionRef {
    id: String,
    title: String,
    level: u64,
    page_start: u32,
}

#[derive(Debug, Clone, Serialize)]
struct DocumentAstNode {
    id: String,
    #[serde(rename = "type")]
    node_type: String,
    page_start: u32,
    page_end: u32,
    element_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    chunk_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    visual_enrichment_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bounding_boxes: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_role: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    section_path: Vec<DocumentAstSectionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    continued_from_section_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    caption_links: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    caption_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    table: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    formula: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chart: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visual_enrichment: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<DocumentAstNode>>,
}

#[derive(Debug, Clone, Default)]
struct DocumentAstStats {
    node_count: usize,
    section_count: usize,
    paragraph_count: usize,
    list_item_count: usize,
    caption_count: usize,
    header_count: usize,
    footer_count: usize,
    section_context_node_count: usize,
    cross_page_section_context_count: usize,
    caption_link_count: usize,
    table_count: usize,
    image_count: usize,
    figure_count: usize,
    chart_count: usize,
    formula_count: usize,
    diagram_count: usize,
    visual_enrichment_count: usize,
    visual_enrichment_kind_counts: BTreeMap<String, usize>,
    max_depth: usize,
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

fn ast_children_at_path<'a>(
    children: &'a mut Vec<DocumentAstNode>,
    path: &[usize],
) -> &'a mut Vec<DocumentAstNode> {
    let Some((&index, tail)) = path.split_first() else {
        return children;
    };
    ast_children_at_path(children[index].children.get_or_insert_with(Vec::new), tail)
}

fn ast_section_ref(node: &DocumentAstNode) -> DocumentAstSectionRef {
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

fn ast_aggregate(node: &mut DocumentAstNode, depth: usize) -> DocumentAstStats {
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

/// Build the v3.0.14 Document AST from the same semantic element and chunk
/// projections used by the public response. Optional provider-backed visual
/// enrichments attach to matching element targets or become page-local visual
/// nodes when the target element is not present on the page.
pub fn build_document_ast(
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

    let mut document_sections = Vec::<DocumentAstSectionRef>::new();
    let mut page_nodes = Vec::new();
    let mut caption_link_budget = AstCaptionLinkBudget::new(MAX_AST_CAPTION_LINK_COMPARISONS);
    for page in &selected_pages {
        let mut page_children = Vec::<DocumentAstNode>::new();
        let mut page_section_stack = Vec::<(u64, Vec<usize>)>::new();
        let page_elements = elements_by_page.remove(page).unwrap_or_default();
        let page_element_ids = page_elements
            .iter()
            .filter_map(|element| element.get("id").and_then(Value::as_str))
            .map(str::to_string)
            .collect::<HashSet<_>>();
        for element in page_elements {
            let element_id = element.get("id").and_then(Value::as_str);
            let enrichment = element_id.and_then(|id| visual_by_target.get(id).copied());
            let Some(mut node) = ast_node_for_element(element, &chunk_index, enrichment) else {
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

        for enrichment in visual_by_page.get(page).into_iter().flatten() {
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

        let mut page_node = DocumentAstNode {
            id: format!("p{page}"),
            node_type: "page".into(),
            page_start: *page,
            page_end: *page,
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
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_twin::test_support::pages;
    use crate::document_twin::*;

    #[test]
    fn document_ast_matches_text_hierarchy_context_aggregation_and_chunk_cache() {
        let selected = vec![
            PageText {
                page: 2,
                text: "continued".into(),
                positioned_items: Vec::new(),
            },
            PageText {
                page: 1,
                text: "chapter".into(),
                positioned_items: Vec::new(),
            },
            PageText {
                page: 2,
                text: "continued".into(),
                positioned_items: Vec::new(),
            },
        ];
        let box_ = json!({"left":10,"bottom":700,"right":200,"top":712});
        let elements = json!([
            {"id":"p1-text-1","type":"text","page":1,"content":"Report","semantic_hint":{"role":"header","confidence":0.82,"signals":["page-top-band"]}},
            {"id":"p1-text-2","type":"text","page":1,"content":"Chapter 1: Intro","bounding_box":box_,"semantic_hint":{"role":"heading","confidence":0.84,"signals":["section-heading-pattern"],"level":1}},
            {"id":"p1-text-3","type":"text","page":1,"content":"Opening paragraph.","semantic_hint":{"role":"paragraph","confidence":0.5,"signals":["default-text"]}},
            {"id":"p1-text-4","type":"text","page":1,"content":"1.1 Scope","semantic_hint":{"role":"heading","confidence":0.84,"signals":["section-heading-pattern"],"level":2}},
            {"id":"p1-text-5","type":"text","page":1,"content":"- bounded item","semantic_hint":{"role":"list_item","confidence":0.92,"signals":["list-prefix"]}},
            {"id":"p2-text-1","type":"text","page":2,"content":"Continued scope.","semantic_hint":{"role":"paragraph","confidence":0.5,"signals":["default-text"]}}
        ]);
        let chunks = json!([
            {"id":"p1-chunk-1","element_ids":["p1-text-2","p1-text-3"]},
            {"id":"p1-chunk-2","element_ids":["p1-text-4","p1-text-5"]},
            {"id":"p2-chunk-3","element_ids":["p2-text-1"]}
        ]);

        let ast = build_document_ast(&selected, &elements, &chunks, &[], &json!([]));
        assert_eq!(ast["summary"]["selected_pages"], json!([1, 2]));
        assert_eq!(ast["summary"]["page_count"], 2);
        assert_eq!(ast["summary"]["node_count"], 9);
        assert_eq!(ast["summary"]["section_count"], 2);
        assert_eq!(ast["summary"]["paragraph_count"], 2);
        assert_eq!(ast["summary"]["list_item_count"], 1);
        assert_eq!(ast["summary"]["header_count"], 1);
        assert_eq!(ast["summary"]["section_context_node_count"], 5);
        assert_eq!(ast["summary"]["cross_page_section_context_count"], 1);
        assert_eq!(ast["summary"]["max_depth"], 5);
        assert!(ast.get("warnings").is_none());

        let root = &ast["root"];
        assert_eq!(root["page_start"], 1);
        assert_eq!(root["page_end"], 2);
        assert_eq!(
            root["element_ids"],
            json!([
                "p1-text-1",
                "p1-text-2",
                "p1-text-3",
                "p1-text-4",
                "p1-text-5",
                "p2-text-1"
            ])
        );
        assert_eq!(
            root["chunk_ids"],
            json!(["p1-chunk-1", "p1-chunk-2", "p2-chunk-3"])
        );
        assert_eq!(root["bounding_boxes"], json!([box_]));
        assert_eq!(root["children"][0]["id"], "p1");
        assert_eq!(root["children"][1]["id"], "p2");

        let heading = &root["children"][0]["children"][1];
        assert_eq!(heading["id"], "p1-text-2-section");
        assert_eq!(heading["section_path"][0]["id"], "p1-text-2-section");
        assert_eq!(heading["children"][1]["id"], "p1-text-4-section");
        assert_eq!(
            heading["children"][1]["section_path"],
            json!([
                {"id":"p1-text-2-section","title":"Chapter 1: Intro","level":1,"page_start":1},
                {"id":"p1-text-4-section","title":"1.1 Scope","level":2,"page_start":1}
            ])
        );
        let continued = &root["children"][1]["children"][0];
        assert_eq!(continued["continued_from_section_id"], "p1-text-4-section");
        assert_eq!(continued["section_path"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn document_ast_omits_empty_aggregates_and_warns_without_headings() {
        let selected = pages(&["Ordinary paragraph."]);
        let elements = json!([{
            "id":"p1-text-1",
            "type":"text",
            "page":1,
            "content":"Ordinary paragraph.",
            "semantic_hint":{"role":"paragraph","confidence":0.5,"signals":["default-text"]}
        }]);
        let ast = build_document_ast(&selected, &elements, &json!([]), &[], &json!([]));
        assert_eq!(ast["root"]["children"][0]["id"], "p1");
        assert!(ast["root"].get("chunk_ids").is_none());
        assert!(ast["root"].get("bounding_boxes").is_none());
        assert_eq!(
            ast["warnings"],
            json!(["No heading hierarchy detected; document_ast uses page-level leaf nodes."])
        );
        assert_eq!(ast["summary"]["node_count"], 3);
        assert_eq!(ast["summary"]["max_depth"], 3);
    }

    #[test]
    fn document_ast_links_captions_to_best_table_and_adds_reverse_ids() {
        let selected = pages(&["caption table"]);
        let caption_box = json!({"left":20,"bottom":20,"right":180,"top":40});
        let table_box = json!({"left":20,"bottom":60,"right":180,"top":140});
        let elements = json!([
            {"id":"caption-1","type":"text","page":1,"content":"Table 1: Primary","bounding_box":caption_box,"semantic_hint":{"role":"caption"}},
            {"id":"caption-2","type":"text","page":1,"content":"Table 2: Secondary","bounding_box":caption_box,"semantic_hint":{"role":"caption"}},
            {"id":"caption-figure","type":"text","page":1,"content":"Figure 1: Not a table","bounding_box":caption_box,"semantic_hint":{"role":"caption"}},
            {"id":"p1-table-1","type":"table","page":1,"bounding_box":table_box,"table":{"rows":[["A","B"]],"rowCount":1,"colCount":2,"confidence":0.9}},
            {"id":"p1-table-2","type":"table","page":1,"bounding_box":table_box,"table":{"rows":[["C","D"]],"rowCount":1,"colCount":2,"confidence":0.9}}
        ]);

        let ast = build_document_ast(&selected, &elements, &json!([]), &[], &json!([]));
        let children = ast["root"]["children"][0]["children"].as_array().unwrap();
        assert_eq!(
            children[0]["caption_links"],
            json!([{
                "node_id":"p1-table-1",
                "element_id":"p1-table-1",
                "type":"table",
                "relation":"below",
                "confidence":0.88,
                "signals":["same-page","horizontal-overlap","caption-below","caption-prefix-table","caption-kind-match"]
            }])
        );
        assert_eq!(children[1]["caption_links"][0]["node_id"], "p1-table-1");
        assert!(children[2].get("caption_links").is_none());
        assert_eq!(
            children[3]["caption_ids"],
            json!(["caption-1", "caption-2"])
        );
        assert!(children[4].get("caption_ids").is_none());
        assert_eq!(ast["summary"]["caption_link_count"], 2);
    }
}
