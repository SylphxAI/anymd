use anymd_core::text_index::extract_page_texts;
use anymd_core::url_fetch::{cleanup_temp_file, fetch_url_to_temp_file};
use anymd_core::{hash_file, ENGINE_NAME, ENGINE_VERSION};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::evidence::attach_evidence;
use crate::ocr_evidence;
use crate::page_selection::selected_pages;
use crate::region_analysis_evidence;
use crate::schema::PdfSource;
use crate::visual_evidence;

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const INSPECT_ROUTE: &str = "rust-pdf-inspect-v1";

pub fn pdf_evidence(args: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| rmcp::ErrorData::invalid_params("operation is required", None))?;

    match operation {
        "inspect" => inspect(args),
        "render_page" => visual_evidence::render_pages(args),
        "extract_regions" => visual_evidence::extract_regions(args),
        "ocr_pages" => ocr_evidence::ocr_pages(args),
        "analyze_regions" => region_analysis_evidence::analyze_regions(args),
        other => Err(rmcp::ErrorData::invalid_params(
            format!("Unsupported pdf_evidence operation: {other}"),
            None,
        )),
    }
}

fn inspect(args: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let sources_value = args
        .get("sources")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| rmcp::ErrorData::invalid_params("sources is required", None))?;

    let parsed_sources: Vec<PdfSource> = sources_value
        .iter()
        .filter_map(|source| serde_json::from_value(source.clone()).ok())
        .collect();

    if parsed_sources.is_empty() {
        return Err(rmcp::ErrorData::invalid_params(
            "sources must include at least one PDF source",
            None,
        ));
    }

    for source in &parsed_sources {
        source
            .validate()
            .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    }

    let mut results = Vec::new();
    let mut temps = Vec::new();
    for source in &parsed_sources {
        let targets = match selected_pages(&source.pages) {
            Ok(value) => value,
            Err(error) => {
                results.push(json!({
                    "source": source.label(),
                    "success": false,
                    "error": error,
                }));
                continue;
            }
        };
        let path_owned: PathBuf = if let Some(path) = source.path.as_ref() {
            PathBuf::from(path)
        } else if let Some(url) = source.url.as_ref() {
            match fetch_url_to_temp_file(url) {
                Ok(temp) => {
                    temps.push(temp.clone());
                    temp
                }
                Err(message) => {
                    results.push(json!({
                        "source": source.label(),
                        "success": false,
                        "error": message,
                    }));
                    continue;
                }
            }
        } else {
            results.push(json!({
                "source": source.label(),
                "success": false,
                "error": "Provide exactly one of path or url for each PDF source.",
            }));
            continue;
        };

        match extract_page_texts(path_owned.as_path(), DEFAULT_MAX_FILE_BYTES) {
            Ok(pages) => {
                let num_pages = pages.len().max(1) as u32;
                let sample_count = args
                    .get("sample_pages")
                    .and_then(Value::as_u64)
                    .unwrap_or(5)
                    .clamp(1, 20) as usize;
                let sampled_pages = sample_pages(num_pages, targets.as_deref(), sample_count);
                let document_signals = match anymd_core::inspect_document_signal_presence(
                    path_owned.as_path(),
                    DEFAULT_MAX_FILE_BYTES,
                    &sampled_pages,
                ) {
                    Ok((_, value)) => value,
                    Err(error) => {
                        results.push(json!({
                            "source": source.label(),
                            "success": false,
                            "error": error.message,
                        }));
                        continue;
                    }
                };
                let page_signals = sampled_pages
                    .iter()
                    .filter_map(|page_number| {
                        pages.get(*page_number as usize - 1).map(|page| {
                            let text_chars = page.trim().chars().count();
                            let text_items = page
                                .lines()
                                .filter(|value| !value.trim().is_empty())
                                .count();
                            json!({
                                "page": page_number,
                                "text_chars": text_chars,
                                "text_items": text_items,
                                "estimated_tokens": text_chars.div_ceil(4),
                                "image_paint_operations": 0,
                                "likely_scanned": false,
                                "low_text_density": text_chars < 80,
                            })
                        })
                    })
                    .collect::<Vec<_>>();
                let profile = if page_signals
                    .iter()
                    .any(|signal| signal["text_chars"].as_u64().unwrap_or(0) >= 80)
                {
                    "digital_text"
                } else {
                    "low_text_or_form"
                };
                let has_structure_tree = document_signals["has_structure_tree"]
                    .as_bool()
                    .unwrap_or(false);
                let mut read_pdf_arguments = json!({
                    "sources": [source],
                    "include_metadata": true,
                    "include_page_count": true,
                    "include_page_geometry": true,
                });
                if has_structure_tree {
                    read_pdf_arguments["include_structure_tree"] = json!(true);
                }
                results.push(json!({
                    "source": source.label(),
                    "success": true,
                    "data": {
                        "profile": profile,
                        "num_pages": num_pages,
                        "sampled_pages": sampled_pages,
                        "page_signals": page_signals,
                        "document_signals": document_signals,
                        "recommendation": {
                            "workflow": if profile == "digital_text" { "agentic_rag" } else { "metadata_review" },
                            "needs_ocr": false,
                            "reason": if profile == "digital_text" {
                                "Sampled pages expose selectable text; the agent document map, citation chunks, semantic hints, table extraction, safety findings, and visual enrichment fusion are the highest-value next read_pdf options when providers are ready."
                            } else {
                                "Sampled pages expose limited text; inspect metadata, forms, attachments, structure, and selected pages before running a heavier extraction."
                            },
                            "read_pdf_arguments": read_pdf_arguments,
                        },
                        "route": INSPECT_ROUTE,
                        "engine": {
                            "name": ENGINE_NAME,
                            "version": ENGINE_VERSION,
                        },
                    }
                }));
            }
            Err(error) => {
                results.push(json!({
                    "source": source.label(),
                    "success": false,
                    "error": error.message,
                }));
            }
        }
    }

    for temp in temps {
        cleanup_temp_file(temp.as_path());
    }

    let source_hash = parsed_sources
        .iter()
        .find_map(|source| source.path.as_ref())
        .and_then(|path| hash_file(PathBuf::from(path).as_path(), DEFAULT_MAX_FILE_BYTES).ok())
        .map(|hash| hash.source_hash);

    let structured = attach_evidence(
        "pdf_evidence",
        Some("inspect"),
        &parsed_sources,
        INSPECT_ROUTE,
        source_hash,
        Vec::new(),
        json!({ "results": results }),
    );

    Ok(CallToolResult::structured(structured))
}

fn sample_pages(total: u32, targets: Option<&[u32]>, max: usize) -> Vec<u32> {
    if total == 0 || max == 0 {
        return Vec::new();
    }
    let max = max.min(20);
    let mut values = targets
        .map(|pages| {
            pages
                .iter()
                .copied()
                .filter(|page| (1..=total).contains(page))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if targets.is_none() {
        if u64::from(total) <= max as u64 {
            return (1..=total).collect();
        }
        if max == 1 {
            return vec![1];
        }
        return (0..max)
            .map(|index| {
                let numerator = index as u64 * u64::from(total - 1);
                let denominator = (max - 1) as u64;
                1 + ((numerator + denominator / 2) / denominator) as u32
            })
            .collect();
    }
    values.sort_unstable();
    values.dedup();
    if values.len() <= max {
        return values;
    }
    if max == 1 {
        return values.into_iter().take(1).collect();
    }
    let mut selected = (0..max)
        .filter_map(|index| {
            let numerator = index * (values.len() - 1);
            let denominator = max - 1;
            let position = (numerator + denominator / 2) / denominator;
            values.get(position).copied()
        })
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure_fixture() -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential/v3014-structure-v1.pdf")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn sampling_matches_frozen_even_spacing_and_target_normalization() {
        assert_eq!(sample_pages(100, None, 5), vec![1, 26, 51, 75, 100]);
        assert_eq!(
            sample_pages(12, Some(&[12, 2, 8, 6, 4, 10, 2, 99]), 3),
            vec![2, 8, 12]
        );
        assert_eq!(sample_pages(12, Some(&[9, 1, 9]), 5), vec![1, 9]);
        assert_eq!(
            sample_pages(u32::MAX, None, 5),
            vec![1, 1_073_741_825, 2_147_483_648, 3_221_225_472, u32::MAX]
        );
        assert_eq!(sample_pages(u32::MAX, None, 1), vec![1]);
        assert!(sample_pages(u32::MAX, None, 0).is_empty());
        let maximally_requested = sample_pages(u32::MAX, None, usize::MAX);
        assert_eq!(maximally_requested.len(), 20);
        assert_eq!(maximally_requested.first(), Some(&1));
        assert_eq!(maximally_requested.last(), Some(&u32::MAX));
    }

    #[test]
    fn inspect_samples_a_compact_max_u32_range_without_expanding_it_unboundedly() {
        for range in ["1-4294967295", "1--5", "1-   "] {
            let result = pdf_evidence(json!({
                "operation":"inspect",
                "sources":[{"path":structure_fixture(),"pages":range}],
                "sample_pages":20
            }))
            .unwrap();
            let payload = result.structured_content.unwrap();
            assert_eq!(payload["results"][0]["success"], true, "{range}");
            assert_eq!(
                payload["results"][0]["data"]["sampled_pages"],
                json!([1, 2]),
                "{range}"
            );
        }
    }

    #[test]
    fn inspect_rejects_max_plus_one_raw_targets_before_file_io() {
        let result = pdf_evidence(json!({
            "operation":"inspect",
            "sources":[{"path":"definitely-missing.pdf","pages":"1-10001,20000"}],
            "sample_pages":5
        }))
        .unwrap();
        let payload = result.structured_content.unwrap();
        assert_eq!(payload["results"][0]["success"], false);
        assert!(payload["results"][0]["error"]
            .as_str()
            .unwrap()
            .contains("10001 selected pages"));
    }
}
