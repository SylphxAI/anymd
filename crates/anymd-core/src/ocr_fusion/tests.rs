use serde_json::json;

use super::{fuse_ocr_outcomes, OcrPage, OcrWord, SourceOcrOutcome, OCR_STUB_WARNING};
use crate::document_twin::PageText;
use crate::read_pdf::{
    EngineInfo, ReadPdfData, ReadPdfResponse, ReadPdfSourceResult, StructuredFusionContext,
};

fn response() -> ReadPdfResponse {
    ReadPdfResponse {
        profile: "agent_document_twin",
        results: vec![ReadPdfSourceResult {
            source: "duplicate.pdf".into(),
            success: true,
            error: None,
            data: Some(ReadPdfData {
                num_pages: None,
                info: None,
                metadata: None,
                full_text: Some("selectable".into()),
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
                document_map: Some(json!({"layers":[],"pages":[{"page":2}],"routing":{}})),
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
                visual_enrichment_candidates: None,
                warnings: Some(vec![OCR_STUB_WARNING.into()]),
                route: "test".into(),
                engine: EngineInfo {
                    name: "test",
                    version: "test",
                },
                ocr_candidate_pages: vec![2],
                structured_fusion_context: None,
            }),
        }],
    }
}

#[test]
fn fuses_by_ordinal_and_preserves_selectable_text() {
    let mut response = response();
    fuse_ocr_outcomes(
        &mut response,
        vec![SourceOcrOutcome {
            source_index: 0,
            pages: vec![OcrPage {
                page: 2,
                text: "A😀".into(),
                confidence: Some(0.914),
                words: Some(vec![OcrWord {
                    text: "A".into(),
                    confidence: Some(0.9),
                    bounding_box: Some(json!({"left":1,"bottom":2,"right":3,"top":4})),
                    ..Default::default()
                }]),
                language: None,
                provider: "command".into(),
                source_render_evidence_id: "render-2".into(),
                source_render_scale: Some(2.0),
                source_render_width: Some(100),
                source_render_height: Some(200),
                provenance: json!({"engine":"external-command","source":"ocr-provider"}),
                warnings: None,
            }],
            warnings: vec!["render warning".into()],
            error: None,
        }],
    );
    let data = response.results[0].data.as_ref().unwrap();
    assert_eq!(data.full_text.as_deref(), Some("selectable"));
    assert_eq!(
        data.ocr_text_layer.as_ref().unwrap()["profile"],
        "ocr_text_layer"
    );
    assert_eq!(
        data.ocr_text_layer.as_ref().unwrap()["summary"]["text_chars"],
        3
    );
    assert_eq!(
        data.document_map.as_ref().unwrap()["routing"]["ocr_applied_pages"],
        json!([2])
    );
    assert!(!data
        .warnings
        .as_ref()
        .unwrap()
        .iter()
        .any(|warning| warning == OCR_STUB_WARNING));
}

#[test]
fn provider_failure_omits_layer_and_keeps_source_successful() {
    let mut response = response();
    fuse_ocr_outcomes(
        &mut response,
        vec![SourceOcrOutcome {
            source_index: 0,
            pages: Vec::new(),
            warnings: Vec::new(),
            error: Some("provider failed".into()),
        }],
    );
    let result = &response.results[0];
    assert!(result.success);
    let data = result.data.as_ref().unwrap();
    assert!(data.ocr_text_layer.is_none());
    assert_eq!(
        data.warnings.as_ref().unwrap(),
        &["OCR text layer unavailable: provider failed"]
    );
    let map = data.document_map.as_ref().unwrap();
    // Layout-derived needs_ocr_pages is preserved; fusion only clears applied pages.
    assert!(map["routing"].get("needs_ocr_pages").is_none());
    assert_eq!(map["routing"]["ocr_applied_pages"], json!([]));
    assert!(!map["layers"]
        .as_array()
        .unwrap()
        .contains(&json!("ocr_text_layer")));
}

#[test]
fn repeated_fusion_replaces_layer_without_duplicate_map_or_warning_entries() {
    let mut response = response();
    let outcome = SourceOcrOutcome {
        source_index: 0,
        pages: vec![OcrPage {
            page: 2,
            text: "replacement".into(),
            confidence: None,
            words: None,
            language: None,
            provider: "command".into(),
            source_render_evidence_id: "render-2".into(),
            source_render_scale: None,
            source_render_width: None,
            source_render_height: None,
            provenance: json!({"engine":"external-command","source":"ocr-provider"}),
            warnings: None,
        }],
        warnings: vec!["render warning".into()],
        error: None,
    };
    fuse_ocr_outcomes(&mut response, vec![outcome.clone()]);
    fuse_ocr_outcomes(&mut response, vec![outcome]);
    let data = response.results[0].data.as_ref().unwrap();
    assert_eq!(
        data.document_map.as_ref().unwrap()["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| **layer == "ocr_text_layer")
            .count(),
        1
    );
    assert_eq!(
        data.warnings
            .as_ref()
            .unwrap()
            .iter()
            .filter(|warning| warning.as_str() == "render warning")
            .count(),
        1
    );
}

#[test]
fn boxed_ocr_table_rebuilds_every_table_derived_surface() {
    let mut response = response();
    let data = response.results[0].data.as_mut().unwrap();
    data.structured_fusion_context = Some(StructuredFusionContext {
        pages: vec![PageText {
            page: 2,
            text: String::new(),
            positioned_items: Vec::new(),
        }],
        total_pages: 2,
        page_geometry: None,
        selectable_tables: json!([]),
        semantic_hints: true,
        emit_markdown: true,
        emit_html: true,
        emit_chunks: true,
        emit_elements: true,
        emit_tables: true,
        emit_document_ast: true,
        emit_document_map: true,
        safety: json!([]),
        layout: json!([]),
        trust: None,
        accessibility: None,
        visual_candidates: json!([]),
    });
    fuse_ocr_outcomes(
        &mut response,
        vec![SourceOcrOutcome {
            source_index: 0,
            pages: vec![OcrPage {
                page: 2,
                text: "Metric Value\nRevenue 24%".into(),
                confidence: None,
                words: Some(
                    vec![
                        ("Metric", 40, 700, 88, 710),
                        ("Value", 160, 700, 202, 710),
                        ("Revenue", 40, 680, 100, 690),
                        ("24%", 160, 680, 184, 690),
                    ]
                    .into_iter()
                    .map(|(text, left, bottom, right, top)| OcrWord {
                        text: text.into(),
                        confidence: None,
                        reading_order: None,
                        region_type: None,
                        layout_confidence: None,
                        bounding_box: Some(
                            json!({"left":left,"bottom":bottom,"right":right,"top":top}),
                        ),
                    })
                    .collect(),
                ),
                language: None,
                provider: "command".into(),
                source_render_evidence_id: "render-2".into(),
                source_render_scale: Some(2.0),
                source_render_width: Some(100),
                source_render_height: Some(100),
                provenance: json!({"engine":"external-command","source":"ocr-provider"}),
                warnings: None,
            }],
            warnings: Vec::new(),
            error: None,
        }],
    );
    let data = response.results[0].data.as_ref().unwrap();
    assert_eq!(
        data.tables.as_ref().unwrap()[0]["rows"][1],
        json!(["Revenue", "24%"])
    );
    assert_eq!(
        data.elements.as_ref().unwrap()[0]["provenance"]["source"],
        "ocr-table-detector"
    );
    assert_eq!(data.chunks.as_ref().unwrap()[0]["strategy"], "table");
    assert!(data
        .markdown
        .as_deref()
        .unwrap()
        .contains("| Metric | Value |"));
    assert!(data.html.as_deref().unwrap().contains("<table"));
    assert_eq!(
        data.document_ast.as_ref().unwrap()["summary"]["table_count"],
        1
    );
    assert_eq!(
        data.document_map.as_ref().unwrap()["summary"]["table_element_count"],
        1
    );
    assert_eq!(
        data.document_map.as_ref().unwrap()["routing"]["ocr_applied_pages"],
        json!([2])
    );

    fuse_ocr_outcomes(
        &mut response,
        vec![SourceOcrOutcome {
            source_index: 0,
            pages: Vec::new(),
            warnings: Vec::new(),
            error: Some("later provider failure".into()),
        }],
    );
    let reset = response.results[0].data.as_ref().unwrap();
    assert_eq!(reset.tables.as_ref().unwrap(), &json!([]));
    assert_eq!(
        reset.document_ast.as_ref().unwrap()["summary"]["table_count"],
        0
    );
    assert_eq!(
        reset.document_map.as_ref().unwrap()["routing"]["ocr_applied_pages"],
        json!([])
    );
}
