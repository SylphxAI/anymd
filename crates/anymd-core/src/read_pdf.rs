//! Pure-Rust read_pdf for anymd (local path + SSRF-safe URL).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::read_result_cache::{shared_cache, ReadResultCache};
use crate::text_index::extract_pdf_text_from_document;
use crate::url_fetch::{
    cleanup_source_temp_file as cleanup_temp_file,
    url_source_to_temp_file as fetch_url_to_temp_file,
};

mod extract;
mod pages;
mod presets;
mod structured;
mod types;

use extract::{auto_enabled, build_data, requires_text_extraction, BuildSignals};
use pages::{evenly_sample_pages, select_pages};
use presets::{apply_read_preset, json_has_explicit_read_options, resolved_read_preset};
pub(crate) use structured::rebuild_structured_outputs;
pub use types::*;

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

fn validate_source(source: &ReadPdfSource) -> Result<(), ReadPdfError> {
    let has_path = source.path.as_ref().is_some_and(|value| !value.is_empty());
    let has_url = source.url.as_ref().is_some_and(|value| !value.is_empty());
    match (has_path, has_url) {
        (true, false) | (false, true) => Ok(()),
        (false, false) => Err(ReadPdfError::invalid_params(
            "Provide exactly one of path or url for each PDF source.",
        )),
        (true, true) => Err(ReadPdfError::invalid_params(
            "Provide exactly one of path or url for each PDF source.",
        )),
    }
}

fn read_source(
    source: &ReadPdfSource,
    input: &ReadPdfInput,
    cache: &ReadResultCache,
) -> ReadPdfSourceResult {
    if let Err(error) = validate_source(source) {
        return ReadPdfSourceResult {
            source: source
                .path
                .clone()
                .or_else(|| source.url.clone())
                .unwrap_or_else(|| "unknown".into()),
            success: false,
            error: Some(error.message),
            data: None,
        };
    }

    if let Some(path) = source.path.as_ref().filter(|p| !p.is_empty()) {
        let path_buf = PathBuf::from(path);
        return match read_local_pdf_filtered(path_buf.as_path(), input, &source.pages, path, cache)
        {
            Ok(result) => result,
            Err(error) => ReadPdfSourceResult {
                source: path.clone(),
                success: false,
                error: Some(error.message),
                data: None,
            },
        };
    }

    if let Some(url) = source.url.as_ref().filter(|u| !u.is_empty()) {
        match fetch_url_to_temp_file(url) {
            Ok(temp) => {
                let result =
                    read_local_pdf_filtered(temp.as_path(), input, &source.pages, url, cache);
                cleanup_temp_file(temp.as_path());
                match result {
                    Ok(mut ok) => {
                        ok.source = url.clone();
                        ok
                    }
                    Err(error) => ReadPdfSourceResult {
                        source: url.clone(),
                        success: false,
                        error: Some(error.message),
                        data: None,
                    },
                }
            }
            Err(message) => ReadPdfSourceResult {
                source: url.clone(),
                success: false,
                error: Some(message),
                data: None,
            },
        }
    } else {
        ReadPdfSourceResult {
            source: "unknown".into(),
            success: false,
            error: Some("Provide exactly one of path or url for each PDF source.".into()),
            data: None,
        }
    }
}

fn read_local_pdf_filtered(
    path: &Path,
    input: &ReadPdfInput,
    pages_spec: &Option<Value>,
    source_label: &str,
    cache: &ReadResultCache,
) -> Result<ReadPdfSourceResult, ReadPdfError> {
    if let Some(mut cached) = cache.get(path, input, pages_spec) {
        // Preserve the caller's source label (path or original display form).
        cached.source = source_label.to_string();
        return Ok(cached);
    }
    let parsed = crate::cos_document::ParsedPdf::load(path, DEFAULT_MAX_FILE_BYTES)?;
    let requires_text = requires_text_extraction(input);
    let (pages, mut pdf_info) = if requires_text {
        let extracted = extract_pdf_text_from_document(&parsed.document)?;
        (extracted.pages, extracted.info)
    } else {
        (
            (0..parsed.pages.len().max(1))
                .map(|_| crate::text_index::ExtractedPageText {
                    text: String::new(),
                    items: Vec::new(),
                    positioned_items: Vec::new(),
                })
                .collect(),
            crate::text_index::read_pdf_info(&parsed.document),
        )
    };
    // Encrypt dictionary is removed after empty-password decrypt; restore
    // pdf.js EncryptFilterName from pre-decrypt encryption facts when needed.
    if pdf_info.encrypt_filter_name.is_none() {
        if let Some(filter_name) = parsed
            .encryption_facts
            .as_ref()
            .and_then(|facts| facts.filter_name.clone())
        {
            pdf_info.encrypt_filter_name = Some(filter_name);
        }
    }
    pdf_info.is_linearized = parsed.is_linearized;
    let total_pages = parsed.pages.len().max(1) as u32;
    let explicit_pages = crate::pages::parse_page_spec::<ReadPdfError>(pages_spec, true)?;
    let auto_pages = if explicit_pages.is_none()
        && auto_enabled(input)
        && input.auto_detail.as_deref().unwrap_or("balanced") != "full"
    {
        Some(evenly_sample_pages(
            total_pages,
            input.sample_pages.unwrap_or(5),
        ))
    } else {
        None
    };
    let requested_pages = explicit_pages.as_deref().or(auto_pages.as_deref());
    let (selected, invalid_pages) = select_pages(&pages, requested_pages);
    let selected_page_numbers = selected.iter().map(|page| page.page).collect::<Vec<_>>();
    let auto = auto_enabled(input);
    let want_geometry = input.include_page_geometry
        || auto
        || input.include_semantic_hints
        || input.include_document_map
        || input.include_document_ast
        || input.include_visual_enrichments
        || input.include_trust_report;
    let want_private_a11y = input.include_accessibility_report
        || (auto
            && matches!(
                input.auto_detail.as_deref().unwrap_or("balanced"),
                "balanced" | "full"
            ));
    let want_annotations = input.include_annotations
        || input.include_trust_report
        || (auto
            && matches!(
                input.auto_detail.as_deref().unwrap_or("balanced"),
                "balanced" | "full"
            ))
        || want_private_a11y;
    let signals = crate::page_signals::extract_page_signals(
        &parsed.document,
        &parsed.pages,
        &selected_page_numbers,
        want_geometry,
        want_annotations,
    );
    let geometry = (!signals.geometry.is_empty()).then(|| json!(signals.geometry));
    let annotations = (!signals.annotations.is_empty()).then(|| json!(signals.annotations));
    let auto_full = auto && input.auto_detail.as_deref() == Some("full");
    let catalog_signals = crate::catalog_signals::extract_catalog_signals(
        &parsed.document,
        parsed.encryption_facts,
        total_pages,
        crate::catalog_signals::CatalogSignalRequest {
            page_labels: input.include_page_labels || auto_full,
            permissions: input.include_permissions || auto_full || want_private_a11y,
            outline: input.include_outline || auto_full || want_private_a11y,
        },
    );
    let form_attachment_signals = crate::form_attachment_signals::extract_form_attachment_signals(
        &parsed.document,
        &parsed.pages,
        input.include_form_fields || auto_full || want_private_a11y,
        input.include_attachments || auto_full,
    );
    let want_private_structure = input.include_structure_tree
        || input.include_accessibility_report
        || (auto
            && matches!(
                input.auto_detail.as_deref().unwrap_or("balanced"),
                "balanced" | "full"
            ));
    let checked_structure = want_private_structure.then(|| {
        crate::structure_signals::extract_structure_trees_checked(
            &parsed.document,
            &parsed.pages,
            &selected_page_numbers,
        )
    });
    let accessibility_structure_valid = match checked_structure.as_ref() {
        None | Some(Ok(None)) => true,
        Some(Ok(Some(value))) => value.complete,
        Some(Err(())) => false,
    };
    let structure_extraction = checked_structure.and_then(Result::ok).flatten();
    let structure_trees = structure_extraction
        .as_ref()
        .and_then(|value| (!value.trees.is_empty()).then(|| json!(value.trees)));
    let accessibility_structure_trees = structure_extraction
        .as_ref()
        .filter(|value| value.complete)
        .and_then(|value| (!value.trees.is_empty()).then(|| json!(value.trees)));
    let image_signals = if input.include_images {
        crate::image_signals::extract_image_signals(
            &parsed.document,
            &parsed.pages,
            &selected_page_numbers,
        )
    } else {
        crate::image_signals::ImageSignals::default()
    };
    let mut signal_warnings = signals.warnings;
    signal_warnings.extend(form_attachment_signals.warnings);
    signal_warnings.extend(image_signals.warnings);
    if !invalid_pages.is_empty() {
        signal_warnings.push(format!(
            "Requested page numbers {} exceed total pages ({total_pages}).",
            invalid_pages
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let data = build_data(
        &selected,
        total_pages,
        input,
        Some(&pdf_info),
        explicit_pages.is_some(),
        BuildSignals {
            page_geometry: geometry,
            annotations,
            page_labels: catalog_signals.page_labels.map(|value| json!(value)),
            permissions: catalog_signals.permissions.map(|value| json!(value)),
            mark_info: catalog_signals.mark_info.map(|value| json!(value)),
            outline: catalog_signals.outline.map(|value| json!(value)),
            form_fields: form_attachment_signals
                .form_fields
                .map(|value| json!(value)),
            attachments: form_attachment_signals
                .attachments
                .map(|value| json!(value)),
            structure_trees,
            accessibility_structure_trees,
            accessibility_structure_valid,
            images: input.include_images.then(|| json!(image_signals.images)),
            has_catalog_metadata: parsed
                .document
                .catalog()
                .ok()
                .and_then(|catalog| catalog.get(b"Metadata").ok())
                .is_some(),
            warnings: signal_warnings,
        },
    );
    let result = ReadPdfSourceResult {
        source: source_label.to_string(),
        success: true,
        error: None,
        data: Some(data),
    };
    cache.store(path, input, pages_spec, &result);
    Ok(result)
}

pub fn read_pdf(input: &ReadPdfInput) -> Result<ReadPdfResponse, ReadPdfError> {
    read_pdf_with_cache(input, shared_cache())
}

fn read_pdf_with_cache(
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
mod tests;
