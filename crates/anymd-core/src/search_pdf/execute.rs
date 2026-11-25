use serde_json::{json, Value};

use crate::text_index::search_pdf_text_pages;
use crate::url_fetch::cleanup_source_temp_file;

use super::{
    request::validate_request,
    result::{failed_source, format_result},
    source::{resolve_source, source_label},
    SearchPdfError, SearchPdfInput, SearchPdfResponse, DEFAULT_CONTEXT_CHARS, DEFAULT_MAX_MATCHES,
    DEFAULT_MAX_PAGES,
};

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

pub fn search_pdf(input: &SearchPdfInput) -> Result<SearchPdfResponse, SearchPdfError> {
    validate_request(input)?;

    let case_sensitive = input.case_sensitive.unwrap_or(false);
    let whole_word = input.whole_word.unwrap_or(false);
    let max_pages = input.max_pages.unwrap_or(DEFAULT_MAX_PAGES);
    let max_matches = input.max_matches_per_source.unwrap_or(DEFAULT_MAX_MATCHES);
    let context_chars = input.context_chars.unwrap_or(DEFAULT_CONTEXT_CHARS);

    let mut results = Vec::new();
    let mut temps = Vec::new();
    for source in &input.sources {
        let label = source_label(source);
        let path = match resolve_source(source, &mut temps) {
            Ok(path) => path,
            Err(message) => {
                results.push(failed_source(&label, message));
                continue;
            }
        };

        let page_filter = crate::pages::parse_page_spec::<SearchPdfError>(&source.pages, false)?;
        match search_pdf_text_pages(
            path.as_path(),
            DEFAULT_MAX_FILE_BYTES,
            &input.query,
            case_sensitive,
            whole_word,
            page_filter.as_deref(),
            max_pages,
            max_matches,
            context_chars,
        ) {
            Ok(search) => {
                let prefer_speed = input.prefer_speed.unwrap_or(false);
                let include_ocr = input.include_ocr_text_layer.unwrap_or(false);
                // current TS prefer_speed uses the rust-text-index route only when OCR is off:
                // matches omit geometry and emit the speed-route warning.
                let speed_route = prefer_speed && !include_ocr;
                results.push(format_result(
                    search,
                    &label,
                    page_filter,
                    max_pages,
                    max_matches,
                    speed_route,
                ));
            }
            Err(error) => results.push(failed_source(&label, error.message)),
        }
    }

    for temp in temps {
        cleanup_source_temp_file(temp.as_path());
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
        return Err(SearchPdfError::invalid_request(format!(
            "All PDF sources failed search: {errors}"
        )));
    }

    Ok(SearchPdfResponse {
        profile: "pdf_search_results",
        search_options: json!({
            "query": input.query,
            "case_sensitive": case_sensitive,
            "whole_word": whole_word,
            "max_pages": max_pages,
            "max_matches_per_source": max_matches,
            "context_chars": context_chars,
            "include_ocr_text_layer": input.include_ocr_text_layer.unwrap_or(false),
            "prefer_speed": input.prefer_speed.unwrap_or(false),
        }),
        results,
    })
}

pub fn search_pdf_from_value(input: &Value) -> Result<SearchPdfResponse, SearchPdfError> {
    let parsed: SearchPdfInput = serde_json::from_value(input.clone()).map_err(|error| {
        SearchPdfError::invalid_params(format!("Invalid search_pdf input: {error}"))
    })?;
    search_pdf(&parsed)
}
