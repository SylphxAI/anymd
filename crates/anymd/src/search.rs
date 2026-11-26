use anymd_core::{
    fuse_search_ocr_outcomes, hash_file, search_pdf_from_value, SearchPdfErrorCode,
    SearchPdfResponse, SEARCH_PDF_ROUTE,
};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::evidence::attach_evidence;
use crate::ocr_evidence::{run_read_ocr, ReadOcrOptions};
use crate::page_selection::selected_pages;
use crate::schema::SearchPdfArgs;
use crate::visual_evidence::{materialize_read_source, MaterializedSource};

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_OCR_SOURCES_PER_REQUEST: usize = 32;

enum OcrSearchOutcome {
    Complete(SearchPdfResponse, Vec<MaterializedSource>, bool),
    AllSourcesFailed(String),
}

fn admit_ocr_source_count(count: usize) -> Result<(), rmcp::ErrorData> {
    if count <= MAX_OCR_SOURCES_PER_REQUEST {
        return Ok(());
    }
    Err(rmcp::ErrorData::invalid_params(
        format!(
            "search_pdf OCR accepts at most {MAX_OCR_SOURCES_PER_REQUEST} sources per request."
        ),
        None,
    ))
}

pub fn search_pdf(args_value: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let args: SearchPdfArgs = serde_json::from_value(args_value.clone()).map_err(|error| {
        rmcp::ErrorData::invalid_params(format!("Invalid search_pdf arguments: {error}"), None)
    })?;

    args.validate()
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    if args.sources.is_empty() {
        return Err(rmcp::ErrorData::invalid_params(
            "sources must include at least one PDF source.",
            None,
        ));
    }

    let ocr_requested = args.include_ocr_text_layer.unwrap_or(false);
    if ocr_requested {
        admit_ocr_source_count(args.sources.len())?;
    }

    let map_error = |error: anymd_core::SearchPdfError| match error.code {
        SearchPdfErrorCode::InvalidParams => rmcp::ErrorData::invalid_params(error.message, None),
        SearchPdfErrorCode::InvalidRequest | SearchPdfErrorCode::ExtractionFailed => {
            rmcp::ErrorData::invalid_request(error.message, None)
        }
    };

    let (response, materialized, has_provider_ocr) = if ocr_requested {
        match search_with_ocr(&args_value, &args).map_err(map_error)? {
            OcrSearchOutcome::Complete(response, materialized, has_provider_ocr) => {
                (response, materialized, has_provider_ocr)
            }
            OcrSearchOutcome::AllSourcesFailed(message) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(message)]));
            }
        }
    } else {
        (
            search_pdf_from_value(&args_value).map_err(map_error)?,
            Vec::new(),
            false,
        )
    };

    let source_hash = materialized
        .first()
        .and_then(|source| hash_file(source.path(), DEFAULT_MAX_FILE_BYTES).ok())
        .or_else(|| {
            args.sources
                .iter()
                .find_map(|source| source.path.as_ref())
                .and_then(|path| {
                    hash_file(PathBuf::from(path).as_path(), DEFAULT_MAX_FILE_BYTES).ok()
                })
        })
        .map(|hash| hash.source_hash);

    let structured = attach_evidence(
        "search_pdf",
        None,
        &args.sources,
        SEARCH_PDF_ROUTE,
        source_hash,
        Vec::new(),
        serde_json::to_value(&response).map_err(|error| {
            rmcp::ErrorData::internal_error(
                format!("Failed to serialize search_pdf: {error}"),
                None,
            )
        })?,
    );
    let mut structured = structured;
    if has_provider_ocr {
        structured["evidence"]["confidence"] = json!("provider-dependent");
    }

    Ok(CallToolResult::structured(structured))
}

fn search_with_ocr(
    args_value: &Value,
    args: &SearchPdfArgs,
) -> Result<OcrSearchOutcome, anymd_core::SearchPdfError> {
    let mut materialized: Vec<Option<MaterializedSource>> =
        (0..args.sources.len()).map(|_| None).collect();
    let mut results = Vec::with_capacity(args.sources.len());
    let mut search_options = None;

    for (source_index, source) in args.sources.iter().enumerate() {
        let original_label = source.label();
        if let Err(error) = selected_pages(&source.pages) {
            results.push(json!({
                "source": original_label,
                "success": false,
                "error": format!("Invalid page specification for source {original_label}: {error}"),
            }));
            continue;
        }
        let owner = match materialize_read_source(source_index, source) {
            Ok(owner) => owner,
            Err(error) => {
                results.push(json!({
                    "source": original_label,
                    "success": false,
                    "error": error,
                }));
                continue;
            }
        };
        let mut source_args = args_value.clone();
        source_args["sources"] = json!([{
            "path": owner.path().to_string_lossy(),
            "pages": serde_json::to_value(&source.pages).unwrap_or(Value::Null),
        }]);
        source_args["include_ocr_text_layer"] = json!(false);
        match search_pdf_from_value(&source_args) {
            Ok(mut source_response) => {
                let mut result = source_response.results.remove(0);
                result["source"] = json!(original_label);
                results.push(result);
                search_options.get_or_insert(source_response.search_options);
                materialized[source_index] = Some(owner);
            }
            Err(error) if error.code == SearchPdfErrorCode::InvalidParams => return Err(error),
            Err(error) => results.push(json!({
                "source": original_label,
                "success": false,
                "error": error.message,
            })),
        }
    }

    if results
        .iter()
        .all(|result| result.get("success") == Some(&Value::Bool(false)))
    {
        let errors = results
            .iter()
            .filter_map(|result| result.get("error").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("; ");
        return Ok(OcrSearchOutcome::AllSourcesFailed(format!(
            "All PDF sources failed search: {errors}"
        )));
    }

    let mut options = search_options.expect("at least one successful search has options");
    options["include_ocr_text_layer"] = json!(true);
    let mut response = SearchPdfResponse {
        profile: "pdf_search_results",
        search_options: options,
        results,
    };
    let ocr_sources = response
        .results
        .iter()
        .enumerate()
        .filter_map(|(source_index, result)| {
            if result.get("success").and_then(Value::as_bool) != Some(true)
                || result.get("truncated").and_then(Value::as_bool) == Some(true)
            {
                return None;
            }
            let matches = result.get("matches")?.as_array()?.len();
            let cap = response.search_options["max_matches_per_source"].as_u64()? as usize;
            if matches >= cap {
                return None;
            }
            let pages = result
                .get("searched_pages")?
                .as_array()?
                .iter()
                .filter_map(Value::as_u64)
                .filter_map(|page| u32::try_from(page).ok())
                .collect::<Vec<_>>();
            materialized[source_index]
                .as_ref()
                .map(|source| (source, pages))
        })
        .collect::<Vec<_>>();
    let outcomes = if ocr_sources.is_empty() {
        Vec::new()
    } else {
        run_read_ocr(&ocr_sources, ReadOcrOptions::default())
    };
    let has_provider_ocr = outcomes.iter().any(|outcome| !outcome.pages.is_empty());
    fuse_search_ocr_outcomes(&mut response, outcomes)?;
    let owners = materialized.into_iter().flatten().collect();
    Ok(OcrSearchOutcome::Complete(
        response,
        owners,
        has_provider_ocr,
    ))
}

#[cfg(test)]
mod tests;
