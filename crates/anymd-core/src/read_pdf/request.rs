//! Batch request validation and per-source result aggregation.

use crate::read_result_cache::ReadResultCache;

use super::source::read_source;
use super::types::{ReadPdfError, ReadPdfInput, ReadPdfResponse};

pub(super) fn read_pdf_with_cache(
    input: &ReadPdfInput,
    cache: &ReadResultCache,
) -> Result<ReadPdfResponse, ReadPdfError> {
    if input.sources.is_empty() {
        return Err(ReadPdfError::invalid_params(
            "sources must include at least one PDF source.",
        ));
    }

    if let Some(profile) = input.profile.as_deref() {
        if !matches!(profile, "fast" | "quality" | "research") {
            return Err(ReadPdfError::invalid_params(
                "profile must be one of: fast, quality, research",
            ));
        }
    }
    if let Some(detail) = input.auto_detail.as_deref() {
        if !matches!(detail, "fast" | "balanced" | "full") {
            return Err(ReadPdfError::invalid_params(
                "auto_detail must be one of: fast, balanced, full",
            ));
        }
    }
    if let Some(sample) = input.sample_pages {
        if !(1..=20).contains(&sample) {
            return Err(ReadPdfError::invalid_params(
                "sample_pages must be an integer between 1 and 20",
            ));
        }
    }
    if let Some(max_vis) = input.max_visual_enrichments {
        if max_vis < 1 {
            return Err(ReadPdfError::invalid_params(
                "max_visual_enrichments must be >= 1 when provided",
            ));
        }
    }
    for source in &input.sources {
        crate::pages::parse_page_spec::<ReadPdfError>(&source.pages, true)?;
    }

    let mut results = Vec::new();
    for source in &input.sources {
        results.push(read_source(source, input, cache));
    }

    if results.iter().all(|result| !result.success) {
        let errors = results
            .iter()
            .filter_map(|result| result.error.as_deref())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ReadPdfError::invalid_request(format!(
            "All PDF sources failed to process: {errors}"
        )));
    }

    Ok(ReadPdfResponse {
        profile: "pdf_read_results",
        results,
    })
}
