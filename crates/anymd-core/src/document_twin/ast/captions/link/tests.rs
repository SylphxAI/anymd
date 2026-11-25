use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::super::{
    ast_link_captions_on_page, AstCaptionLinkBudget, MAX_AST_CAPTION_LINK_COMPARISONS,
};
use super::{ast_caption_link, AstCaptionLinkNode};
use crate::document_twin::ast::{nodes::ast_node_for_element, DocumentAstNode};
use crate::text_index::TextBoundingBox;

#[test]
fn caption_link_geometry_matches_ts_boundaries_and_kind_filter() {
    let node =
        |id: &str, node_type: &str, text: Option<&str>, box_: TextBoundingBox| AstCaptionLinkNode {
            id: id.into(),
            node_type: node_type.into(),
            text: text.map(str::to_string),
            element_id: id.into(),
            bounding_box: Some(box_),
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
