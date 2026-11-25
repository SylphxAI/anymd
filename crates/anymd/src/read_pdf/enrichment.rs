use anymd_core::{
    fuse_ocr_outcomes, fuse_visual_outcomes, ReadPdfResponse, SourceVisualOutcome,
    VISUAL_NO_CANDIDATE_WARNING,
};
use serde_json::Value;

use crate::ocr_evidence::{run_read_ocr, ReadOcrOptions};
use crate::region_analysis_evidence;
use crate::visual_evidence::MaterializedSource;

pub(super) fn enrich_ocr(
    payload: &mut ReadPdfResponse,
    materialized: &[Option<MaterializedSource>],
    ocr_requested: bool,
) {
    let ocr_sources = payload
        .results
        .iter()
        .enumerate()
        .filter_map(|(source_index, result)| {
            let pages = result.data.as_ref()?.ocr_candidate_pages.clone();
            (!pages.is_empty()).then(|| {
                materialized[source_index]
                    .as_ref()
                    .map(|source| (source, pages))
            })?
        })
        .collect::<Vec<_>>();
    let mut outcomes = if ocr_sources.is_empty() {
        Vec::new()
    } else {
        run_read_ocr(&ocr_sources, ReadOcrOptions::default())
    };
    if ocr_requested {
        outcomes.extend(
            payload
                .results
                .iter()
                .enumerate()
                .filter(|(_, result)| {
                    result
                        .data
                        .as_ref()
                        .is_some_and(|data| data.ocr_candidate_pages.is_empty())
                })
                .map(|(source_index, _)| anymd_core::SourceOcrOutcome {
                    source_index,
                    pages: Vec::new(),
                    warnings: Vec::new(),
                    error: None,
                }),
        );
    }
    fuse_ocr_outcomes(payload, outcomes);
}

pub(super) fn enrich_visual(
    payload: &mut ReadPdfResponse,
    materialized: &[Option<MaterializedSource>],
    args: &Value,
) -> bool {
    let visual_requested = args
        .get("include_visual_enrichments")
        .and_then(Value::as_bool)
        == Some(true);
    let mut visual_outcomes: Vec<SourceVisualOutcome> = Vec::new();
    let mut has_provider_visual = false;
    if visual_requested {
        let provider_ready = region_analysis_evidence::command_provider_ready();
        if provider_ready {
            let max_visual = args
                .get("max_visual_enrichments")
                .and_then(Value::as_u64)
                .map(|value| value as u32)
                .unwrap_or(8)
                .clamp(1, 100);
            let mut analyze_sources = Vec::new();
            for (source_index, result) in payload.results.iter().enumerate() {
                let Some(data) = result.data.as_ref().filter(|_| result.success) else {
                    continue;
                };
                let candidates = data
                    .visual_enrichment_candidates
                    .as_ref()
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if candidates.is_empty() {
                    visual_outcomes.push(SourceVisualOutcome {
                        source_index,
                        provider_ready: true,
                        enrichments: Vec::new(),
                        warnings: vec![VISUAL_NO_CANDIDATE_WARNING.into()],
                        error: None,
                    });
                    continue;
                }
                let Some(owner) = materialized[source_index].as_ref() else {
                    visual_outcomes.push(SourceVisualOutcome {
                        source_index,
                        provider_ready: true,
                        enrichments: Vec::new(),
                        warnings: Vec::new(),
                        error: Some("Visual enrichment source materialization is missing.".into()),
                    });
                    continue;
                };
                analyze_sources.push((
                    source_index,
                    result.source.clone(),
                    owner.path().to_path_buf(),
                    candidates,
                ));
            }
            if !analyze_sources.is_empty() {
                visual_outcomes.extend(
                    region_analysis_evidence::analyze_visual_candidates_for_sources(
                        &analyze_sources,
                        max_visual,
                    ),
                );
            }
        }
        fuse_visual_outcomes(payload, visual_outcomes);
        has_provider_visual = payload.results.iter().any(|result| {
            result
                .data
                .as_ref()
                .and_then(|data| data.visual_enrichments.as_ref())
                .and_then(Value::as_array)
                .is_some_and(|values| !values.is_empty())
        });
    }
    has_provider_visual
}
