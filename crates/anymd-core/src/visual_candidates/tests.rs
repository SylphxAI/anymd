use serde_json::json;

use super::geometry::parse_box;
use super::*;

fn element(id: &str, kind: &str, page: u32, box_: Value) -> Value {
    json!({ "id": id, "type": kind, "page": page, "bounding_box": box_ })
}

fn caption(id: &str, content: &str, box_: Value) -> Value {
    json!({ "id": id, "type": "text", "page": 1, "content": content,
        "bounding_box": box_, "semantic_hint": { "role": "caption" } })
}

fn box_(left: f64, bottom: f64, right: f64, top: f64) -> Value {
    json!({ "left": left, "bottom": bottom, "right": right, "top": top })
}

fn geometry() -> Vec<Value> {
    vec![json!({ "page": 1, "width": 600, "height": 800, "rotation": 0 })]
}

#[test]
fn direct_targets_preserve_order_and_apply_minimum_one_and_cap() {
    let elements = vec![
        element("image-1", "image", 1, box_(10.0, 10.0, 100.0, 100.0)),
        element("table-1", "table", 1, box_(10.0, 120.0, 100.0, 200.0)),
        element("image-2", "image", 1, box_(10.0, 220.0, 100.0, 300.0)),
    ];
    let one = select_visual_enrichment_candidates(&elements, None, 0);
    assert_eq!(
        one.candidates
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["image-1"]
    );
    let two = select_visual_enrichment_candidates(&elements, None, 2);
    assert_eq!(
        two.candidates
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["image-1", "table-1"]
    );
}

#[test]
fn invalid_and_missing_boxes_fail_closed() {
    let elements = vec![
        json!({ "id": "missing", "type": "image", "page": 1 }),
        element("reversed", "table", 1, box_(20.0, 0.0, 10.0, 10.0)),
        element(
            "string",
            "image",
            1,
            json!({ "left": "NaN", "bottom": 0, "right": 10, "top": 10 }),
        ),
    ];
    assert!(select_visual_enrichment_candidates(&elements, None, 8)
        .candidates
        .is_empty());
}

#[test]
fn caption_selects_above_before_side_and_falls_back_when_unaccompanied() {
    let bounds = box_(0.0, 0.0, 600.0, 800.0);
    let elements = vec![
        caption(
            "fig-1",
            "Figure 1: result",
            box_(250.0, 100.0, 350.0, 120.0),
        ),
        element("above", "text", 1, box_(200.0, 140.0, 400.0, 300.0)),
        element("side", "text", 1, box_(360.0, 100.0, 500.0, 120.0)),
        caption(
            "eq-1",
            "Equation (A) energy",
            box_(250.0, 600.0, 350.0, 620.0),
        ),
    ];
    let output = select_visual_enrichment_candidates(&elements, Some(&geometry()), 8);
    assert_eq!(output.candidates.len(), 2);
    assert!(output.candidates[0]
        .candidate_signals
        .contains(&"caption-target-above".into()));
    assert_eq!(
        output.candidates[0].region.bounding_box,
        VisualBoundingBox {
            left: 179.0,
            bottom: 79.0,
            right: 421.0,
            top: 321.0
        }
    );
    assert!(output.candidates[1]
        .candidate_signals
        .contains(&"caption-region-expansion".into()));
    assert!(output.candidates[1].region.bounding_box.top <= 800.0);
    assert!(parse_box(&bounds).is_some());
}

#[test]
fn caption_uses_side_group_when_no_vertical_group_exists() {
    let elements = vec![
        caption(
            "chart-1",
            "Chart 2 - trend",
            box_(250.0, 300.0, 350.0, 330.0),
        ),
        element("left", "text", 1, box_(100.0, 300.0, 220.0, 330.0)),
    ];
    let output = select_visual_enrichment_candidates(&elements, Some(&geometry()), 8);
    assert!(output.candidates[0]
        .candidate_signals
        .contains(&"caption-target-left".into()));
}

#[test]
fn matching_nearby_direct_target_suppresses_caption_region() {
    let elements = vec![
        caption(
            "table-caption",
            "Table 1: values",
            box_(100.0, 100.0, 300.0, 120.0),
        ),
        element("table", "table", 1, box_(100.0, 130.0, 300.0, 260.0)),
    ];
    let output = select_visual_enrichment_candidates(&elements, Some(&geometry()), 8);
    assert_eq!(output.candidates.len(), 1);
    assert_eq!(output.candidates[0].id, "table");
}

#[test]
fn element_scan_cap_admits_exact_and_rejects_cap_plus_one() {
    let exact = (0..MAX_VISUAL_CANDIDATE_SCAN_ELEMENTS)
        .map(|index| json!({ "id": format!("x-{index}"), "type": "text", "page": 1, "content": "plain" }))
        .collect::<Vec<_>>();
    assert!(!select_visual_enrichment_candidates(&exact, None, 8).limited);
    let mut over = exact;
    over.push(json!({ "id": "over", "type": "text", "page": 1, "content": "plain" }));
    let output = select_visual_enrichment_candidates(&over, None, 8);
    assert!(output.limited);
    assert_eq!(output.warnings[0].limit, VisualCandidateLimit::Elements);
}

#[test]
fn page_geometry_cap_admits_exact_and_rejects_cap_plus_one() {
    let exact = (1..=MAX_VISUAL_CANDIDATE_PAGE_GEOMETRY)
        .map(|page| json!({ "page": page, "width": 600, "height": 800 }))
        .collect::<Vec<_>>();
    assert!(!select_visual_enrichment_candidates(&[], Some(&exact), 8).limited);
    let mut over = exact;
    over.push(
        json!({ "page": MAX_VISUAL_CANDIDATE_PAGE_GEOMETRY + 1, "width": 600, "height": 800 }),
    );
    let output = select_visual_enrichment_candidates(&[], Some(&over), 8);
    assert!(output.limited);
    assert_eq!(output.warnings[0].limit, VisualCandidateLimit::PageGeometry);
}

#[test]
fn comparison_cap_admits_exact_and_rejects_next_comparison() {
    let mut exact = AdmissionBudget::default();
    for _ in 0..MAX_VISUAL_CANDIDATE_COMPARISONS {
        exact.compare().expect("exact cap admitted");
    }
    assert_eq!(exact.comparisons, MAX_VISUAL_CANDIDATE_COMPARISONS);
    assert!(exact.compare().is_err());

    let mut exact_request = vec![caption(
        "formula",
        "Formula 1: x",
        box_(10.0, 10.0, 80.0, 30.0),
    )];
    exact_request.extend((0..(MAX_VISUAL_CANDIDATE_COMPARISONS / 2)).map(|index| {
        element(
            &format!("exact-image-{index}"),
            "image",
            1,
            box_(200.0, 200.0, 210.0, 210.0),
        )
    }));
    let exact_output =
        select_visual_enrichment_candidates(&exact_request, Some(&geometry()), exact_request.len());
    assert!(!exact_output.limited);
    assert_eq!(exact_output.comparisons, MAX_VISUAL_CANDIDATE_COMPARISONS);

    let mut elements = vec![caption(
        "formula",
        "Formula 1: x",
        box_(10.0, 10.0, 80.0, 30.0),
    )];
    elements.extend((0..=MAX_VISUAL_CANDIDATE_COMPARISONS).map(|index| {
        element(
            &format!("image-{index}"),
            "image",
            1,
            box_(200.0, 200.0, 210.0, 210.0),
        )
    }));
    let output = select_visual_enrichment_candidates(&elements, Some(&geometry()), elements.len());
    assert!(output.limited);
    assert!(output.candidates.is_empty());
    assert_eq!(output.warnings[0].limit, VisualCandidateLimit::Comparisons);
}
