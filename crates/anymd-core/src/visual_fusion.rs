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

use serde_json::Value;

use crate::read_pdf::ReadPdfResponse;

mod document_map;
mod source;

#[cfg(test)]
mod tests;

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
        source::fuse_source(data, &source_label, outcome);
    }
}
