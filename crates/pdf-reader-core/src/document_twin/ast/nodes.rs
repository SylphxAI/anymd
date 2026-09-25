//! Document AST node construction from elements and visual enrichments.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::DocumentAstNode;

fn visual_text_from_enrichment(enrichment: &Value) -> Option<String> {
    for key in ["markdown", "text", "description"] {
        if let Some(value) = enrichment.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    if let Some(formula) = enrichment.get("formula") {
        for key in ["latex", "text"] {
            if let Some(value) = formula.get(key).and_then(Value::as_str) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    if let Some(value) = enrichment.pointer("/chart/summary").and_then(Value::as_str) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn visual_enrichment_node_type(kind: &str) -> String {
    match kind {
        "figure" | "chart" | "formula" | "diagram" => kind.to_string(),
        _ => "visual_region".into(),
    }
}

fn apply_visual_enrichment(node: &mut DocumentAstNode, enrichment: &Value) {
    let enrichment_id = enrichment
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !enrichment_id.is_empty() {
        node.visual_enrichment_ids = vec![enrichment_id];
    }
    if let Some(confidence) = enrichment
        .get("confidence")
        .filter(|value| !value.is_null())
    {
        node.confidence = Some(confidence.clone());
    }
    if let Some(text) = visual_text_from_enrichment(enrichment) {
        // Keep selectable table/image element text; enrichment text is still
        // available on the nested visual_enrichment payload.
        let keep_element_text = matches!(node.node_type.as_str(), "table" | "image")
            && node.text.as_ref().is_some_and(|value| !value.is_empty());
        if !keep_element_text {
            node.text = Some(text);
        }
    }
    if let Some(formula) = enrichment.get("formula").cloned() {
        node.formula = Some(formula);
    }
    if let Some(chart) = enrichment.get("chart").cloned() {
        node.chart = Some(chart);
    }
    node.visual_enrichment = Some(enrichment.clone());
}

pub(super) fn node_for_visual_enrichment(enrichment: &Value) -> Option<DocumentAstNode> {
    let id = enrichment.get("id")?.as_str()?.to_string();
    let page = u32::try_from(enrichment.get("page")?.as_u64()?).ok()?;
    let kind = enrichment
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let target = enrichment
        .get("target_element_id")
        .and_then(Value::as_str)
        .unwrap_or(id.as_str())
        .to_string();
    let bounding_boxes = enrichment
        .get("source_bounding_box")
        .cloned()
        .into_iter()
        .collect::<Vec<_>>();
    let confidence = enrichment
        .get("confidence")
        .filter(|value| !value.is_null())
        .cloned();
    Some(DocumentAstNode {
        id,
        node_type: visual_enrichment_node_type(kind),
        page_start: page,
        page_end: page,
        element_ids: vec![target],
        visual_enrichment_ids: enrichment
            .get("id")
            .and_then(Value::as_str)
            .map(|value| vec![value.to_string()])
            .unwrap_or_default(),
        chunk_ids: Vec::new(),
        bounding_boxes,
        title: None,
        text: visual_text_from_enrichment(enrichment),
        level: None,
        confidence,
        semantic_role: None,
        section_path: Vec::new(),
        continued_from_section_id: None,
        caption_links: Vec::new(),
        caption_ids: Vec::new(),
        table: None,
        image: None,
        formula: enrichment.get("formula").cloned(),
        chart: enrichment.get("chart").cloned(),
        visual_enrichment: Some(enrichment.clone()),
        children: None,
    })
}

pub(super) fn ast_node_for_element(
    element: &Value,
    chunk_index: &BTreeMap<String, Vec<String>>,
    visual_enrichment: Option<&Value>,
) -> Option<DocumentAstNode> {
    let id = element.get("id")?.as_str()?.to_string();
    let page = u32::try_from(element.get("page")?.as_u64()?).ok()?;
    let element_type = element.get("type")?.as_str()?;
    let bounding_boxes = element
        .get("bounding_box")
        .cloned()
        .into_iter()
        .collect::<Vec<_>>();
    let confidence = element
        .get("confidence")
        .filter(|value| !value.is_null())
        .cloned();

    if element_type == "text" {
        let text = element.get("content")?.as_str()?.to_string();
        let role = element
            .pointer("/semantic_hint/role")
            .and_then(Value::as_str)
            .unwrap_or("paragraph");
        let node_type = match role {
            "heading" => "section",
            "list_item" => "list_item",
            "caption" | "header" | "footer" => role,
            _ => "paragraph",
        };
        let is_section = node_type == "section";
        let mut node = DocumentAstNode {
            id: if is_section {
                format!("{id}-section")
            } else {
                id.clone()
            },
            node_type: node_type.to_string(),
            page_start: page,
            page_end: page,
            element_ids: vec![id.clone()],
            visual_enrichment_ids: Vec::new(),
            chunk_ids: chunk_index.get(&id).cloned().unwrap_or_default(),
            bounding_boxes,
            title: is_section.then(|| text.clone()),
            text: Some(text),
            level: is_section.then(|| {
                element
                    .pointer("/semantic_hint/level")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
            }),
            confidence,
            semantic_role: Some(role.to_string()),
            section_path: Vec::new(),
            continued_from_section_id: None,
            caption_links: Vec::new(),
            caption_ids: Vec::new(),
            table: None,
            image: None,
            formula: None,
            chart: None,
            visual_enrichment: None,
            children: is_section.then(Vec::new),
        };
        if let Some(enrichment) = visual_enrichment {
            apply_visual_enrichment(&mut node, enrichment);
        }
        return Some(node);
    }

    if element_type == "table" {
        let source = element.get("table")?;
        let mut table = json!({
            "rows": source.get("rows"),
            "rowCount": source.get("rowCount"),
            "colCount": source.get("colCount"),
            "confidence": source.get("confidence"),
        });
        for key in ["quality", "continuation", "provenance"] {
            if let Some(value) = source.get(key).cloned() {
                table[key] = value;
            }
        }
        let text = source
            .get("rows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|row| {
                row.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut node = DocumentAstNode {
            id: id.clone(),
            node_type: "table".into(),
            page_start: page,
            page_end: page,
            element_ids: vec![id.clone()],
            visual_enrichment_ids: Vec::new(),
            chunk_ids: chunk_index.get(&id).cloned().unwrap_or_default(),
            bounding_boxes,
            title: None,
            text: Some(text),
            level: None,
            confidence,
            semantic_role: None,
            section_path: Vec::new(),
            continued_from_section_id: None,
            caption_links: Vec::new(),
            caption_ids: Vec::new(),
            table: Some(table),
            image: None,
            formula: None,
            chart: None,
            visual_enrichment: None,
            children: None,
        };
        if let Some(enrichment) = visual_enrichment {
            apply_visual_enrichment(&mut node, enrichment);
        }
        return Some(node);
    }

    if element_type == "image" {
        let mut node = DocumentAstNode {
            id: id.clone(),
            node_type: "image".into(),
            page_start: page,
            page_end: page,
            element_ids: vec![id.clone()],
            visual_enrichment_ids: Vec::new(),
            chunk_ids: chunk_index.get(&id).cloned().unwrap_or_default(),
            bounding_boxes,
            title: None,
            text: None,
            level: None,
            confidence,
            semantic_role: None,
            section_path: Vec::new(),
            continued_from_section_id: None,
            caption_links: Vec::new(),
            caption_ids: Vec::new(),
            table: None,
            image: element.get("image").cloned(),
            formula: None,
            chart: None,
            visual_enrichment: None,
            children: None,
        };
        if let Some(enrichment) = visual_enrichment {
            apply_visual_enrichment(&mut node, enrichment);
        }
        return Some(node);
    }

    None
}
