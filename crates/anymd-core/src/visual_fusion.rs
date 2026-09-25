//! Deterministic fusion of configured-command visual enrichment outcomes into
//! `read_pdf` results and Document Map projections.
//!
//! Provider execution stays in the MCP server boundary. This module only:
//! - removes the provider-not-configured stub once readiness is established
//! - retains candidates on soft provider failure
//! - emits top-level enrichments only when analyses exist
//! - updates Document Map enrichment layers, page indexes/counts, arrays, and
//!   kind summaries
//! - synchronizes Document Map warnings with the public result

use serde_json::{json, Map, Value};

use crate::read_pdf::ReadPdfResponse;

pub const VISUAL_STUB_WARNING: &str =
    "Visual enrichment skipped: analyze_regions provider is not_configured.";

pub const VISUAL_NO_CANDIDATE_WARNING: &str = "Visual enrichment requested, but no table, image, or caption-derived visual regions with bounding boxes were available.";

/// Provider outcome for one source ordinal.
///
/// Source identity is ordinal by design: duplicate source labels are valid.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceVisualOutcome {
    pub source_index: usize,
    /// True when the command region-analysis provider is configured and ready.
    pub provider_ready: bool,
    /// Bound enrichments. Empty when the provider failed or produced nothing.
    pub enrichments: Vec<Value>,
    pub warnings: Vec<String>,
    /// Soft source-local provider failure message (without the public wrapper).
    pub error: Option<String>,
}

fn append_unique_warning(warnings: &mut Option<Vec<String>>, warning: String) {
    let values = warnings.get_or_insert_with(Vec::new);
    if !values.contains(&warning) {
        values.push(warning);
    }
}

fn remove_stub_warning(warnings: &mut Option<Vec<String>>) {
    if let Some(values) = warnings {
        values.retain(|warning| warning != VISUAL_STUB_WARNING);
        if values.is_empty() {
            *warnings = None;
        }
    }
}

fn insert_layer_after(layers: &mut Vec<Value>, layer: &str, after: &str) {
    let layer_value = json!(layer);
    if layers.iter().any(|entry| entry.as_str() == Some(layer)) {
        return;
    }
    if let Some(index) = layers
        .iter()
        .position(|entry| entry.as_str() == Some(after))
    {
        layers.insert(index + 1, layer_value);
    } else {
        layers.push(layer_value);
    }
}

fn remove_layer(layers: &mut Vec<Value>, layer: &str) {
    layers.retain(|entry| entry.as_str() != Some(layer));
}

fn kind_counts(enrichments: &[Value]) -> Map<String, Value> {
    let mut counts = Map::new();
    for enrichment in enrichments {
        let kind = enrichment
            .get("target_element_type")
            .and_then(Value::as_str)
            .unwrap_or("visual_region");
        let entry = counts.entry(kind.to_string()).or_insert(json!(0));
        if let Some(value) = entry.as_u64() {
            *entry = json!(value + 1);
        }
    }
    counts
}

fn fuse_document_map(map: &mut Value, enrichments: &[Value], warnings: &Option<Vec<String>>) {
    let Some(object) = map.as_object_mut() else {
        return;
    };

    let layers = object
        .entry("layers")
        .or_insert_with(|| json!([]))
        .as_array_mut();
    if let Some(layers) = layers {
        if enrichments.is_empty() {
            remove_layer(layers, "visual_enrichment");
        } else {
            insert_layer_after(layers, "visual_enrichment", "visual_region_candidates");
        }
    }

    let mut indexes_by_page: std::collections::BTreeMap<u64, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, enrichment) in enrichments.iter().enumerate() {
        if let Some(page) = enrichment.get("page").and_then(Value::as_u64) {
            indexes_by_page.entry(page).or_default().push(index);
        }
    }

    if let Some(map_pages) = object.get_mut("pages").and_then(Value::as_array_mut) {
        for map_page in map_pages {
            let Some(page_number) = map_page.get("page").and_then(Value::as_u64) else {
                continue;
            };
            let Some(page_object) = map_page.as_object_mut() else {
                continue;
            };
            let indexes = indexes_by_page
                .get(&page_number)
                .cloned()
                .unwrap_or_default();
            page_object.insert("visual_enrichment_indexes".into(), json!(indexes));
            page_object.insert(
                "visual_enrichment_count".into(),
                json!(indexes_by_page.get(&page_number).map_or(0, Vec::len)),
            );
        }
    }

    object.insert("visual_enrichments".into(), json!(enrichments));

    let summary = object.entry("summary").or_insert_with(|| json!({}));
    if let Some(summary) = summary.as_object_mut() {
        summary.insert("visual_enrichment_count".into(), json!(enrichments.len()));
        summary.insert(
            "visual_enrichment_kind_counts".into(),
            Value::Object(kind_counts(enrichments)),
        );
    }

    match warnings {
        Some(values) if !values.is_empty() => {
            object.insert("warnings".into(), json!(values));
        }
        _ => {
            object.remove("warnings");
        }
    }
}

fn rebuild_document_ast_if_needed(data: &mut crate::read_pdf::ReadPdfData) {
    let Some(context) = data.structured_fusion_context.clone() else {
        return;
    };
    if !context.emit_document_ast {
        return;
    }
    use crate::document_twin::{
        build_citation_chunks, build_document_ast, build_elements_with_tables_and_geometry,
    };
    let tables = if context.emit_tables {
        data.tables
            .clone()
            .unwrap_or_else(|| context.selectable_tables.clone())
    } else {
        context.selectable_tables.clone()
    };
    let semantic_elements = build_elements_with_tables_and_geometry(
        &context.pages,
        &tables,
        true,
        context.page_geometry.as_ref(),
    );
    let internal_chunks = build_citation_chunks(&semantic_elements, true);
    let warnings = data.warnings.clone().unwrap_or_default();
    let enrichments = data.visual_enrichments.clone().unwrap_or_else(|| json!([]));
    data.document_ast = Some(build_document_ast(
        &context.pages,
        &semantic_elements,
        &internal_chunks,
        &warnings,
        &enrichments,
    ));
}

/// Fuse normalized provider outcomes into their matching read results.
pub fn fuse_visual_outcomes(response: &mut ReadPdfResponse, outcomes: Vec<SourceVisualOutcome>) {
    for outcome in outcomes {
        let Some(result) = response.results.get_mut(outcome.source_index) else {
            continue;
        };
        let source_label = result.source.clone();
        let Some(data) = result.data.as_mut().filter(|_| result.success) else {
            continue;
        };

        if outcome.provider_ready {
            remove_stub_warning(&mut data.warnings);
        }

        data.visual_enrichments = None;

        if let Some(error) = outcome.error {
            for warning in outcome.warnings {
                append_unique_warning(&mut data.warnings, warning);
            }
            append_unique_warning(
                &mut data.warnings,
                format!("Visual enrichment unavailable for {source_label}: {error}"),
            );
            if let Some(map) = data.document_map.as_mut() {
                fuse_document_map(map, &[], &data.warnings);
            }
            rebuild_document_ast_if_needed(data);
            continue;
        }

        let enrichments = outcome.enrichments;
        if !enrichments.is_empty() {
            data.visual_enrichments = Some(Value::Array(enrichments.clone()));
        }

        for warning in outcome.warnings {
            append_unique_warning(&mut data.warnings, warning);
        }

        if let Some(map) = data.document_map.as_mut() {
            fuse_document_map(map, &enrichments, &data.warnings);
        }
        rebuild_document_ast_if_needed(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_pdf::{EngineInfo, ReadPdfData, ReadPdfSourceResult};

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
}
