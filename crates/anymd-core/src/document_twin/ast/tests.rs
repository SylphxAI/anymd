use serde_json::json;

use super::build_document_ast;
use crate::document_twin::{test_support::pages, PageText};

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
