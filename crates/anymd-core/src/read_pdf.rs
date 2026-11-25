//! Pure-Rust read_pdf for anymd (local path + SSRF-safe URL).

use serde_json::Value;

use crate::read_result_cache::shared_cache;

mod extract;
mod local;
mod pages;
mod presets;
mod request;
mod signals;
mod source;
mod structured;
mod types;

use presets::{apply_read_preset, json_has_explicit_read_options, resolved_read_preset};
use request::read_pdf_with_cache;
pub(crate) use structured::rebuild_structured_outputs;
pub use types::*;

pub fn read_pdf(input: &ReadPdfInput) -> Result<ReadPdfResponse, ReadPdfError> {
    read_pdf_with_cache(input, shared_cache())
}

pub fn read_pdf_from_value(input: &Value) -> Result<ReadPdfResponse, ReadPdfError> {
    let mut parsed: ReadPdfInput = serde_json::from_value(input.clone()).map_err(|error| {
        ReadPdfError::invalid_params(format!("Invalid read_pdf input: {error}"))
    })?;

    // Metadata and page count stay on unless the caller turns them off.
    if input.get("include_metadata").is_none() {
        parsed.include_metadata = true;
    }
    if input.get("include_page_count").is_none() {
        parsed.include_page_count = true;
    }

    let auto_specified = input.get("auto").is_some();
    let legacy_auto = parsed.auto == Some(true);
    let manual = (auto_specified && !legacy_auto)
        || (!auto_specified && json_has_explicit_read_options(input));

    if manual {
        if !auto_specified {
            parsed.auto = Some(false);
        }
    } else {
        let preset = resolved_read_preset(&parsed, legacy_auto);
        apply_read_preset(&mut parsed, input, &preset);
        // Resolved presets read every requested page. Sampling belongs to
        // pdf_evidence inspect, not this path.
        parsed.auto_policy_resolved = true;
    }

    read_pdf(&parsed)
}

#[cfg(test)]
use {
    crate::read_result_cache::ReadResultCache,
    extract::{build_data, requires_text_extraction, BuildSignals},
    local::DEFAULT_MAX_FILE_BYTES,
    pages::{evenly_sample_pages, select_pages},
    serde_json::json,
    source::validate_source,
    std::path::PathBuf,
};

#[cfg(test)]
mod tests;
