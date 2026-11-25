use serde_json::json;

use super::*;
use crate::read_pdf::{EngineInfo, ReadPdfData, ReadPdfResponse, ReadPdfSourceResult};

fn sample_result(source: &str, with_map: bool) -> ReadPdfSourceResult {
    let mut data = ReadPdfData {
        num_pages: Some(7),
        info: None,
        metadata: None,
        full_text: None,
        page_texts: None,
        markdown: None,
        html: None,
        chunks: None,
        elements: None,
        text_layer: None,
        tables: None,
        images: None,
        safety_findings: None,
        layout_diagnostics: None,
        document_map: None,
        document_ast: None,
        trust_report: None,
        accessibility_report: None,
        outline: None,
        annotations: None,
        form_fields: None,
        attachments: None,
        structure_trees: None,
        page_labels: None,
        page_geometry: None,
        permissions: None,
        mark_info: None,
        ocr_text_layer: None,
        visual_enrichments: None,
        visual_enrichment_candidates: Some(json!([{
            "id": "p1-table-1",
            "page": 1,
            "target_element_type": "table"
        }])),
        warnings: Some(vec![VISUAL_STUB_WARNING.into()]),
        route: "pure-rust".into(),
        engine: EngineInfo {
            name: "pdf-reader-core",
            version: "test",
        },
        ocr_candidate_pages: Vec::new(),
        structured_fusion_context: None,
    };
    if with_map {
        data.document_map = Some(json!({
            "layers": [
                "selectable_text",
                "visual_region_candidates",
                "semantic_hints"
            ],
            "pages": [{
                "page": 1,
                "visual_candidate_indexes": [0],
                "visual_candidate_count": 1,
                "visual_enrichment_indexes": [],
                "visual_enrichment_count": 0
            }],
            "visual_enrichment_candidates": data.visual_enrichment_candidates.clone(),
            "visual_enrichments": [],
            "summary": {
                "visual_enrichment_candidate_count": 1,
                "visual_enrichment_count": 0,
                "visual_enrichment_kind_counts": {}
            },
            "warnings": [VISUAL_STUB_WARNING]
        }));
    }
    ReadPdfSourceResult {
        source: source.into(),
        success: true,
        error: None,
        data: Some(data),
    }
}

#[test]
fn fuses_enrichments_and_repairs_document_map_layer_order() {
    let mut response = ReadPdfResponse {
        profile: "pdf_read_results",
        results: vec![sample_result("/tmp/a.pdf", true)],
    };
    fuse_visual_outcomes(
        &mut response,
        vec![SourceVisualOutcome {
            source_index: 0,
            provider_ready: true,
            enrichments: vec![json!({
                "id": "visual-p1-table-1",
                "page": 1,
                "region_id": "p1-table-1",
                "target_element_type": "table",
                "kind": "table"
            })],
            warnings: vec![],
            error: None,
        }],
    );
    let data = response.results[0].data.as_ref().unwrap();
    assert!(data.warnings.is_none());
    assert_eq!(
        data.visual_enrichments.as_ref().unwrap()[0]["id"],
        "visual-p1-table-1"
    );
    let map = data.document_map.as_ref().unwrap();
    assert_eq!(
        map["layers"],
        json!([
            "selectable_text",
            "visual_region_candidates",
            "visual_enrichment",
            "semantic_hints"
        ])
    );
    assert_eq!(map["pages"][0]["visual_enrichment_indexes"], json!([0]));
    assert_eq!(map["pages"][0]["visual_enrichment_count"], 1);
    assert_eq!(map["summary"]["visual_enrichment_count"], 1);
    assert_eq!(
        map["summary"]["visual_enrichment_kind_counts"],
        json!({"table": 1})
    );
    assert!(map.get("warnings").is_none());
}

#[test]
fn soft_failure_keeps_candidates_and_discards_partial_enrichments() {
    let mut response = ReadPdfResponse {
        profile: "pdf_read_results",
        results: vec![sample_result("/tmp/a.pdf", true)],
    };
    fuse_visual_outcomes(
        &mut response,
        vec![SourceVisualOutcome {
            source_index: 0,
            provider_ready: true,
            enrichments: vec![json!({"id": "should-not-stick"})],
            warnings: vec![],
            error: Some(
                "Region analysis provider command failed for page 2 region p2-text-2-figure-region."
                    .into(),
            ),
        }],
    );
    let data = response.results[0].data.as_ref().unwrap();
    assert!(data.visual_enrichments.is_none());
    assert!(data.visual_enrichment_candidates.is_some());
    assert_eq!(
        data.warnings.as_ref().unwrap(),
        &vec![
            "Visual enrichment unavailable for /tmp/a.pdf: Region analysis provider command failed for page 2 region p2-text-2-figure-region."
                .to_string()
        ]
    );
    let map = data.document_map.as_ref().unwrap();
    assert_eq!(
        map["layers"],
        json!([
            "selectable_text",
            "visual_region_candidates",
            "semantic_hints"
        ])
    );
    assert_eq!(map["visual_enrichments"], json!([]));
    assert_eq!(map["summary"]["visual_enrichment_count"], 0);
    assert_eq!(
        map["warnings"],
        json!([
            "Visual enrichment unavailable for /tmp/a.pdf: Region analysis provider command failed for page 2 region p2-text-2-figure-region."
        ])
    );
}

#[test]
fn ready_without_candidates_replaces_stub_with_no_candidate_warning() {
    let mut response = ReadPdfResponse {
        profile: "pdf_read_results",
        results: vec![sample_result("/tmp/empty.pdf", false)],
    };
    response.results[0]
        .data
        .as_mut()
        .unwrap()
        .visual_enrichment_candidates = None;
    fuse_visual_outcomes(
        &mut response,
        vec![SourceVisualOutcome {
            source_index: 0,
            provider_ready: true,
            enrichments: vec![],
            warnings: vec![VISUAL_NO_CANDIDATE_WARNING.into()],
            error: None,
        }],
    );
    let data = response.results[0].data.as_ref().unwrap();
    assert_eq!(
        data.warnings.as_ref().unwrap(),
        &vec![VISUAL_NO_CANDIDATE_WARNING.to_string()]
    );
    assert!(data.visual_enrichments.is_none());
}
