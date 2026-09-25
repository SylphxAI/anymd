//! Rust-native search_pdf extraction for anymd.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::text_index::{
    search_literal_text_bounded, search_pdf_text_pages, TextIndexError, TextIndexErrorCode,
    TEXT_INDEX_ROUTE,
};
use crate::url_fetch::{cleanup_temp_file, fetch_url_to_temp_file};
use crate::{SourceOcrOutcome, ENGINE_NAME, ENGINE_VERSION};

pub const SEARCH_PDF_ROUTE: &str = TEXT_INDEX_ROUTE;

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_MAX_PAGES: u32 = 100;
const DEFAULT_MAX_MATCHES: u32 = 50;
const DEFAULT_CONTEXT_CHARS: u32 = 120;
const MAX_SELECTED_PAGES: usize = 10_001;
const MAX_OCR_SEARCH_WORDS: usize = 250_000;
const MAX_OCR_SEARCH_UTF16_UNITS: usize = 2_000_000;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SearchPdfSource {
    pub path: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub pages: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SearchPdfInput {
    pub sources: Vec<SearchPdfSource>,
    pub query: String,
    pub case_sensitive: Option<bool>,
    pub whole_word: Option<bool>,
    pub include_ocr_text_layer: Option<bool>,
    pub max_pages: Option<u32>,
    pub max_matches_per_source: Option<u32>,
    pub context_chars: Option<u32>,
    pub prefer_speed: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchPdfResponse {
    pub profile: &'static str,
    pub search_options: Value,
    pub results: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchPdfErrorCode {
    InvalidParams,
    InvalidRequest,
    ExtractionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPdfError {
    pub code: SearchPdfErrorCode,
    pub message: String,
}

impl SearchPdfError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: SearchPdfErrorCode::InvalidParams,
            message: message.into(),
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: SearchPdfErrorCode::InvalidRequest,
            message: message.into(),
        }
    }
}

impl From<TextIndexError> for SearchPdfError {
    fn from(error: TextIndexError) -> Self {
        match error.code {
            TextIndexErrorCode::InvalidParams => Self::invalid_params(error.message),
            TextIndexErrorCode::InvalidRequest => Self::invalid_request(error.message),
            TextIndexErrorCode::ExtractionFailed => Self {
                code: SearchPdfErrorCode::ExtractionFailed,
                message: error.message,
            },
        }
    }
}

fn validate_source(source: &SearchPdfSource) -> Result<(), SearchPdfError> {
    let has_path = source.path.as_ref().is_some_and(|value| !value.is_empty());
    let has_url = source.url.as_ref().is_some_and(|value| !value.is_empty());
    match (has_path, has_url) {
        (true, false) | (false, true) => Ok(()),
        (false, false) => Err(SearchPdfError::invalid_params(
            "Provide exactly one of path or url for each PDF source.",
        )),
        (true, true) => Err(SearchPdfError::invalid_params(
            "Provide exactly one of path or url for each PDF source.",
        )),
    }
}

fn source_label(source: &SearchPdfSource) -> String {
    source
        .path
        .clone()
        .or_else(|| source.url.clone())
        .unwrap_or_else(|| "unknown".into())
}

fn parse_page_filter(pages_spec: &Option<Value>) -> Result<Option<Vec<u32>>, SearchPdfError> {
    let Some(spec) = pages_spec else {
        return Ok(None);
    };
    let mut wanted = Vec::new();
    if let Some(arr) = spec.as_array() {
        if arr.is_empty() {
            return Err(SearchPdfError::invalid_params(
                "Page specification resulted in an empty set of pages.",
            ));
        }
        for value in arr {
            let page = value
                .as_u64()
                .filter(|page| *page > 0 && *page <= u64::from(u32::MAX));
            let Some(page) = page else {
                return Err(SearchPdfError::invalid_params(
                    "Page numbers in array must be positive integers.",
                ));
            };
            wanted.push(page as u32);
            if wanted.len() > MAX_SELECTED_PAGES {
                return Err(SearchPdfError::invalid_params(
                    "Page specification exceeds the maximum of 10001 selected pages.",
                ));
            }
        }
    } else if let Some(ranges) = spec.as_str() {
        for raw_part in ranges.split(',') {
            let part = raw_part.trim();
            if part.contains('-') {
                let mut segments = part.split('-');
                let start_text = segments.next().unwrap_or_default();
                let end_text = segments.next().unwrap_or_default();
                let start = parse_ts_positive_page(start_text);
                let end = if end_text.is_empty() {
                    start.map(|page| page.saturating_add(10_000))
                } else {
                    parse_ts_positive_page(end_text)
                };
                let (Some(start), Some(end)) = (start, end) else {
                    return Err(SearchPdfError::invalid_params(format!(
                        "Invalid page range values: {part}"
                    )));
                };
                if start > end {
                    return Err(SearchPdfError::invalid_params(format!(
                        "Invalid page range values: {part}"
                    )));
                }
                wanted.extend(start..=end.min(start.saturating_add(10_000)));
            } else {
                let Some(page) = parse_ts_positive_page(part) else {
                    return Err(SearchPdfError::invalid_params(format!(
                        "Invalid page number: {part}"
                    )));
                };
                wanted.push(page);
            }
            if wanted.len() > MAX_SELECTED_PAGES {
                return Err(SearchPdfError::invalid_params(
                    "Page specification exceeds the maximum of 10001 selected pages.",
                ));
            }
        }
    } else {
        return Err(SearchPdfError::invalid_params(
            "Page specification must be a non-empty range string or array of positive integers.",
        ));
    }
    wanted.sort_unstable();
    wanted.dedup();
    if wanted.is_empty() {
        return Err(SearchPdfError::invalid_params(
            "Page specification resulted in an empty set of pages.",
        ));
    }
    Ok(Some(wanted))
}

fn parse_ts_positive_page(value: &str) -> Option<u32> {
    let value = value.trim_start();
    let value = value.strip_prefix('+').unwrap_or(value);
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty())
        .then(|| digits.parse::<u32>().ok())
        .flatten()
        .filter(|page| *page > 0)
}

pub fn search_pdf(input: &SearchPdfInput) -> Result<SearchPdfResponse, SearchPdfError> {
    if input.sources.is_empty() {
        return Err(SearchPdfError::invalid_params(
            "sources must include at least one PDF source.",
        ));
    }
    if input.query.is_empty() {
        return Err(SearchPdfError::invalid_params("query must not be empty"));
    }
    if input.include_ocr_text_layer.unwrap_or(false) {
        return Err(SearchPdfError::invalid_request(
            "search_pdf include_ocr_text_layer requires an OCR provider; pure-Rust search uses the embedded text layer only.",
        ));
    }

    if matches!(input.max_pages, Some(0)) {
        return Err(SearchPdfError::invalid_params("max_pages must be >= 1"));
    }
    if matches!(input.max_matches_per_source, Some(0)) {
        return Err(SearchPdfError::invalid_params(
            "max_matches_per_source must be >= 1",
        ));
    }
    if input.max_pages.is_some_and(|value| value > 1000) {
        return Err(SearchPdfError::invalid_params("max_pages must be <= 1000"));
    }
    if input
        .max_matches_per_source
        .is_some_and(|value| value > 500)
    {
        return Err(SearchPdfError::invalid_params(
            "max_matches_per_source must be <= 500",
        ));
    }
    if input.context_chars.is_some_and(|value| value > 1000) {
        return Err(SearchPdfError::invalid_params(
            "context_chars must be <= 1000",
        ));
    }

    for source in &input.sources {
        validate_source(source)?;
        parse_page_filter(&source.pages)?;
    }

    let case_sensitive = input.case_sensitive.unwrap_or(false);
    let whole_word = input.whole_word.unwrap_or(false);
    let max_pages = input.max_pages.unwrap_or(DEFAULT_MAX_PAGES);
    let max_matches = input.max_matches_per_source.unwrap_or(DEFAULT_MAX_MATCHES);
    let context_chars = input.context_chars.unwrap_or(DEFAULT_CONTEXT_CHARS);

    let mut results = Vec::new();
    let mut temps = Vec::new();
    for source in &input.sources {
        let label = source_label(source);
        let resolved: Result<PathBuf, String> =
            if let Some(path) = source.path.as_ref().filter(|value| !value.is_empty()) {
                Ok(PathBuf::from(path))
            } else if let Some(url) = source.url.as_ref().filter(|value| !value.is_empty()) {
                match fetch_url_to_temp_file(url) {
                    Ok(temp) => {
                        temps.push(temp.clone());
                        Ok(temp)
                    }
                    Err(message) => Err(message),
                }
            } else {
                Err("Provide exactly one of path or url for each PDF source.".into())
            };

        let path = match resolved {
            Ok(path) => path,
            Err(message) => {
                results.push(json!({
                    "source": label,
                    "success": false,
                    "error": message,
                }));
                continue;
            }
        };

        let page_filter = parse_page_filter(&source.pages)?;
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
                let requested_pages =
                    page_filter.unwrap_or_else(|| (1..=search.num_pages).collect());
                let invalid_pages: Vec<u32> = requested_pages
                    .iter()
                    .copied()
                    .filter(|page| *page > search.num_pages)
                    .collect();
                let valid_pages: Vec<u32> = requested_pages
                    .iter()
                    .copied()
                    .filter(|page| *page <= search.num_pages)
                    .collect();
                let searched_pages: Vec<u32> = valid_pages
                    .iter()
                    .copied()
                    .take(max_pages as usize)
                    .collect();
                if searched_pages.is_empty() {
                    results.push(json!({
                        "source": label,
                        "success": false,
                        "error": format!("No valid pages to search for source {}.", source_label(source)),
                    }));
                    continue;
                }
                let prefer_speed = input.prefer_speed.unwrap_or(false);
                let include_ocr = input.include_ocr_text_layer.unwrap_or(false);
                // current TS prefer_speed uses the rust-text-index route only when OCR is off:
                // matches omit geometry and emit the speed-route warning.
                let speed_route = prefer_speed && !include_ocr;
                let matches: Vec<Value> = search
                    .matches
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        if speed_route {
                            return json!({
                                "id": format!("p{}-match-{}", item.page, index + 1),
                                "page": item.page,
                                "text": item.text,
                                "snippet": item.snippet,
                                "match_start": item.match_start,
                                "match_end": item.match_end,
                                "provenance": {
                                    "engine": "rust-text-index",
                                    "source": "text-content",
                                }
                            });
                        }
                        let mut value = json!({
                            "id": format!("p{}-match-{}", item.page, index + 1),
                            "page": item.page,
                            "text": item.text,
                            "snippet": item.snippet,
                            "match_start": item.match_start,
                            "match_end": item.match_end,
                            "text_item_index": item.text_item_index,
                            "provenance": {
                                "engine": item.route,
                                "route": item.route,
                            }
                        });
                        if let Some(bounding_box) = item.bounding_box {
                            value["bounding_box"] = json!(bounding_box);
                        }
                        if let Some(level) = item.bounding_box_level.as_ref() {
                            value["bounding_box_level"] = json!(level);
                        }
                        value
                    })
                    .collect();

                let mut warnings = Vec::new();
                if speed_route {
                    warnings.push(
                        "Search route: rust-text-index via literal PDF text extraction. Bounding boxes are unavailable on this route; use read_pdf or pdf_evidence for geometry-backed evidence."
                            .to_string(),
                    );
                }
                if !invalid_pages.is_empty() {
                    warnings.push(format!(
                        "Requested page numbers {} exceed total pages ({}).",
                        invalid_pages
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", "),
                        search.num_pages
                    ));
                }
                if valid_pages.len() > searched_pages.len() {
                    warnings.push(format!(
                        "Searched first {max_pages} selected pages; skipped {} due to max_pages.",
                        valid_pages[max_pages as usize..]
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if search.truncated {
                    warnings.push(format!(
                        "Search results truncated to {max_matches} matches for this source."
                    ));
                }

                let mut result = json!({
                    "source": label,
                    "success": true,
                    "num_pages": search.num_pages,
                    "searched_pages": searched_pages,
                    "total_matches": matches.len(),
                    "matches": matches,
                    "route": search.route,
                    "page_cache": search.page_cache,
                    "engine": {
                        "name": ENGINE_NAME,
                        "version": ENGINE_VERSION,
                    }
                });
                if search.truncated {
                    result["truncated"] = json!(true);
                }
                if !warnings.is_empty() {
                    result["warnings"] = json!(warnings);
                }
                results.push(result);
            }
            Err(error) => {
                results.push(json!({
                    "source": label,
                    "success": false,
                    "error": error.message,
                }));
            }
        }
    }

    for temp in temps {
        cleanup_temp_file(temp.as_path());
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

#[derive(Default)]
struct OcrSearchBudget {
    words: usize,
    utf16_units: usize,
    exhausted: Option<String>,
}

impl OcrSearchBudget {
    fn exhaust(&mut self, message: String) -> Result<(), String> {
        self.exhausted = Some(message.clone());
        Err(message)
    }

    fn charge(&mut self, words: usize, utf16_units: usize) -> Result<(), String> {
        if let Some(error) = &self.exhausted {
            return Err(error.clone());
        }
        let Some(next_words) = self.words.checked_add(words) else {
            return self.exhaust("OCR search word work overflow.".to_string());
        };
        let Some(next_units) = self.utf16_units.checked_add(utf16_units) else {
            return self.exhaust("OCR search text work overflow.".to_string());
        };
        let error = if next_words > MAX_OCR_SEARCH_WORDS {
            Some(format!(
                "Request exceeds OCR search work limit of {MAX_OCR_SEARCH_WORDS} words."
            ))
        } else if next_units > MAX_OCR_SEARCH_UTF16_UNITS {
            Some(format!(
                "Request exceeds OCR search work limit of {MAX_OCR_SEARCH_UTF16_UNITS} UTF-16 units."
            ))
        } else {
            None
        };
        if let Some(error) = error {
            return self.exhaust(error);
        }
        self.words = next_words;
        self.utf16_units = next_units;
        Ok(())
    }
}

fn append_warning(result: &mut Value, warning: String) {
    let warnings = result
        .as_object_mut()
        .expect("search result is an object")
        .entry("warnings")
        .or_insert_with(|| json!([]));
    if let Some(values) = warnings.as_array_mut() {
        values.push(json!(warning));
    }
}

fn finite_box(value: &Value) -> Option<[f64; 4]> {
    let values = ["left", "bottom", "right", "top"].map(|key| {
        value
            .get(key)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
    });
    let [Some(left), Some(bottom), Some(right), Some(top)] = values else {
        return None;
    };
    Some([left, bottom, right, top])
}

fn merge_boxes(boxes: impl Iterator<Item = [f64; 4]>) -> Option<Value> {
    boxes
        .reduce(|left, right| {
            [
                left[0].min(right[0]),
                left[1].min(right[1]),
                left[2].max(right[2]),
                left[3].max(right[3]),
            ]
        })
        .map(|value| {
            json!({
                "left": value[0],
                "bottom": value[1],
                "right": value[2],
                "top": value[3],
            })
        })
}

/// Fuse normalized server-owned OCR outcomes into search results. This is
/// deliberately provider-neutral: environment, rendering, command execution,
/// and temporary-file ownership remain outside anymd-core.
pub fn fuse_search_ocr_outcomes(
    response: &mut SearchPdfResponse,
    outcomes: Vec<SourceOcrOutcome>,
) -> Result<(), SearchPdfError> {
    let query = response.search_options["query"]
        .as_str()
        .ok_or_else(|| SearchPdfError::invalid_request("Missing search query."))?
        .to_string();
    let case_sensitive = response.search_options["case_sensitive"]
        .as_bool()
        .unwrap_or(false);
    let whole_word = response.search_options["whole_word"]
        .as_bool()
        .unwrap_or(false);
    let max_matches = response.search_options["max_matches_per_source"]
        .as_u64()
        .unwrap_or(u64::from(DEFAULT_MAX_MATCHES)) as usize;
    let context_chars = response.search_options["context_chars"]
        .as_u64()
        .unwrap_or(u64::from(DEFAULT_CONTEXT_CHARS)) as usize;
    let mut budget = OcrSearchBudget::default();

    for outcome in outcomes {
        let Some(result) = response.results.get_mut(outcome.source_index) else {
            continue;
        };
        if result.get("success").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        for warning in outcome.warnings {
            append_warning(result, warning);
        }
        if let Some(error) = outcome.error {
            append_warning(result, format!("OCR search unavailable: {error}"));
            continue;
        }

        let existing = result["matches"].as_array().map_or(0, Vec::len);
        if result.get("truncated").and_then(Value::as_bool) == Some(true) || existing >= max_matches
        {
            continue;
        }
        let mut next_matches = Vec::new();
        let mut truncated = false;
        let mut fusion_failed = false;
        for page in outcome.pages {
            let (search_text, word_ranges) =
                if let Some(words) = page.words.as_ref().filter(|v| !v.is_empty()) {
                    let units = words
                        .iter()
                        .try_fold(0usize, |total, word| {
                            total.checked_add(word.text.encode_utf16().count())
                        })
                        .and_then(|total| total.checked_add(words.len().saturating_sub(1)))
                        .ok_or_else(|| {
                            SearchPdfError::invalid_request("OCR search text work overflow.")
                        })?;
                    if let Err(error) = budget.charge(words.len(), units) {
                        append_warning(result, format!("OCR search unavailable: {error}"));
                        fusion_failed = true;
                        break;
                    }
                    let mut text = String::new();
                    let mut ranges = Vec::with_capacity(words.len());
                    let mut cursor = 0u32;
                    for (index, word) in words.iter().enumerate() {
                        if index > 0 {
                            text.push(' ');
                            cursor = cursor.saturating_add(1);
                        }
                        let start = cursor;
                        text.push_str(&word.text);
                        cursor = cursor.saturating_add(
                            word.text
                                .encode_utf16()
                                .count()
                                .try_into()
                                .unwrap_or(u32::MAX),
                        );
                        ranges.push((
                            start,
                            cursor,
                            word.bounding_box.as_ref().and_then(finite_box),
                        ));
                    }
                    (text, Some(ranges))
                } else {
                    let units = page.text.encode_utf16().count();
                    if let Err(error) = budget.charge(0, units) {
                        append_warning(result, format!("OCR search unavailable: {error}"));
                        fusion_failed = true;
                        break;
                    }
                    (page.text.clone(), None)
                };

            let remaining_with_probe = max_matches
                .saturating_sub(existing + next_matches.len())
                .saturating_add(1);
            let found = match search_literal_text_bounded(
                &search_text,
                &query,
                case_sensitive,
                whole_word,
                context_chars,
                remaining_with_probe,
            ) {
                Ok(found) => found,
                Err(error) => {
                    append_warning(result, format!("OCR search unavailable: {}", error.message));
                    fusion_failed = true;
                    break;
                }
            };
            for item in found {
                if existing + next_matches.len() >= max_matches {
                    truncated = true;
                    break;
                }
                let mut value = json!({
                    "id": format!("p{}-ocr-match-{}", page.page, existing + next_matches.len() + 1),
                    "page": page.page,
                    "text": item.text,
                    "snippet": item.snippet,
                    "match_start": item.start_utf16,
                    "match_end": item.end_utf16,
                    "source_render_evidence_id": page.source_render_evidence_id,
                    "provenance": {
                        "engine": "external-command",
                        "source": "ocr-provider",
                    }
                });
                if let Some(ranges) = &word_ranges {
                    let overlapping = ranges
                        .iter()
                        .enumerate()
                        .filter(|(_, (start, end, _))| {
                            *end > item.start_utf16 && *start < item.end_utf16
                        })
                        .collect::<Vec<_>>();
                    let first_word_index = overlapping.first().map(|(index, _)| *index);
                    if let (Some(first_word_index), Some(box_)) = (
                        first_word_index,
                        merge_boxes(
                            overlapping
                                .into_iter()
                                .filter_map(|(_, (_, _, box_))| *box_),
                        ),
                    ) {
                        value["ocr_word_index"] = json!(first_word_index);
                        value["bounding_box"] = box_;
                        value["bounding_box_level"] = json!("ocr_word");
                    }
                }
                next_matches.push(value);
            }
            if truncated {
                break;
            }
        }

        if fusion_failed {
            next_matches.clear();
            truncated = false;
        }

        if let Some(matches) = result["matches"].as_array_mut() {
            matches.extend(next_matches);
            result["total_matches"] = json!(matches.len());
        }
        if truncated {
            result["truncated"] = json!(true);
            append_warning(
                result,
                format!("Search results truncated to {max_matches} matches for this source."),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_response(query: &str, max_matches: usize, existing: Vec<Value>) -> SearchPdfResponse {
        SearchPdfResponse {
            profile: "pdf_search_results",
            search_options: json!({
                "query": query,
                "case_sensitive": false,
                "whole_word": false,
                "max_matches_per_source": max_matches,
                "context_chars": 2,
                "include_ocr_text_layer": true,
            }),
            results: vec![json!({
                "source": "fixture.pdf",
                "success": true,
                "num_pages": 1,
                "searched_pages": [1],
                "total_matches": existing.len(),
                "matches": existing,
            })],
        }
    }

    fn ocr_page(text: &str, words: Option<Vec<crate::OcrWord>>) -> crate::OcrPage {
        crate::OcrPage {
            page: 1,
            text: text.into(),
            confidence: None,
            words,
            language: None,
            provider: "command".into(),
            source_render_evidence_id: "page-1-render-scale-2".into(),
            source_render_scale: Some(2.0),
            source_render_width: Some(100),
            source_render_height: Some(100),
            provenance: json!({"engine": "external-command", "source": "ocr-provider"}),
            warnings: None,
        }
    }

    fn outcome(page: crate::OcrPage) -> SourceOcrOutcome {
        SourceOcrOutcome {
            source_index: 0,
            pages: vec![page],
            warnings: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn searches_fixture_with_rust_text_index() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/sample.pdf");
        if !fixture.is_file() {
            return;
        }

        let response = search_pdf(&SearchPdfInput {
            sources: vec![SearchPdfSource {
                path: Some(fixture.to_string_lossy().to_string()),
                url: None,
                pages: None,
            }],
            query: "Lorem".into(),
            ..SearchPdfInput::default()
        })
        .expect("search fixture");

        assert_eq!(response.profile, "pdf_search_results");
        assert!(response.results[0]
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false));
        assert_eq!(
            response.results[0].get("route").and_then(Value::as_str),
            Some(SEARCH_PDF_ROUTE)
        );
        assert!(response.results[0].get("warnings").is_none());
        assert!(response.results[0].get("truncated").is_none());
    }

    #[test]
    fn rejects_descending_and_invalid_page_specs() {
        for pages in [
            json!("5-3"),
            json!("0"),
            json!(""),
            json!([]),
            json!([1, 0]),
        ] {
            let error = parse_page_filter(&Some(pages)).expect_err("invalid page spec");
            assert_eq!(error.code, SearchPdfErrorCode::InvalidParams);
        }
        assert_eq!(
            parse_page_filter(&Some(json!("1x"))).expect("TS parseInt-compatible page"),
            Some(vec![1])
        );
    }

    #[test]
    fn enforces_search_limits_at_core_boundary() {
        let source = SearchPdfSource {
            path: Some("/does/not/matter.pdf".into()),
            url: None,
            pages: None,
        };
        for input in [
            SearchPdfInput {
                sources: vec![source.clone()],
                query: "x".into(),
                max_pages: Some(0),
                ..Default::default()
            },
            SearchPdfInput {
                sources: vec![source.clone()],
                query: "x".into(),
                max_matches_per_source: Some(0),
                ..Default::default()
            },
        ] {
            let error = search_pdf(&input).expect_err("zero limit must fail");
            assert_eq!(error.code, SearchPdfErrorCode::InvalidParams);
        }
    }

    #[test]
    fn page_ranges_use_only_the_exact_second_hyphen_segment_like_ts() {
        let open = parse_page_filter(&Some(json!("1--5")))
            .expect("TS treats an empty second segment as open")
            .expect("page filter");
        assert_eq!(open.len(), MAX_SELECTED_PAGES);
        assert_eq!(open.first(), Some(&1));
        assert_eq!(open.last(), Some(&10_001));
        assert_eq!(
            parse_page_filter(&Some(json!("1-2-999"))).expect("third segment ignored"),
            Some(vec![1, 2])
        );
        let whitespace_open = parse_page_filter(&Some(json!("1-   ")))
            .expect("TS trims the whole range part before splitting")
            .expect("page filter");
        assert_eq!(whitespace_open, open);
    }

    #[test]
    fn accepts_whitespace_query_like_v3_0_14_schema_and_handler() {
        let error = search_pdf(&SearchPdfInput {
            sources: vec![SearchPdfSource {
                path: Some("/missing.pdf".into()),
                url: None,
                pages: None,
            }],
            query: " ".into(),
            ..Default::default()
        })
        .expect_err("missing source should fail after query validation");
        assert_eq!(error.code, SearchPdfErrorCode::InvalidRequest);
    }

    #[test]
    fn page_filter_is_applied_before_search_success() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/sample.pdf");
        if !fixture.is_file() {
            return;
        }
        let error = search_pdf(&SearchPdfInput {
            sources: vec![SearchPdfSource {
                path: Some(fixture.to_string_lossy().to_string()),
                url: None,
                pages: Some(json!([9999])),
            }],
            query: "Lorem".into(),
            ..Default::default()
        })
        .expect_err("out-of-range selection must not search page one");
        assert!(error.message.contains("No valid pages to search"));
    }

    #[test]
    fn invalid_page_warning_matches_v3_0_14_wording() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/sample.pdf");
        if !fixture.is_file() {
            return;
        }
        let response = search_pdf(&SearchPdfInput {
            sources: vec![SearchPdfSource {
                path: Some(fixture.to_string_lossy().to_string()),
                url: None,
                pages: Some(json!([1, 99])),
            }],
            query: "Lorem".into(),
            ..Default::default()
        })
        .expect("search fixture");

        assert_eq!(
            response.results[0]["warnings"],
            json!(["Requested page numbers 99 exceed total pages (1)."])
        );
    }

    #[test]
    fn ocr_words_override_page_text_and_preserve_utf16_geometry() {
        let words = vec![
            crate::OcrWord {
                text: "Metric".into(),
                confidence: None,
                bounding_box: Some(json!({"left":40.0,"bottom":700.0,"right":88.0,"top":710.0})),
            },
            crate::OcrWord {
                text: "Value".into(),
                confidence: None,
                bounding_box: Some(json!({"left":120.0,"bottom":700.0,"right":202.0,"top":710.0})),
            },
            crate::OcrWord {
                text: "Revenue".into(),
                confidence: None,
                bounding_box: Some(json!({"left":40.0,"bottom":680.0,"right":100.0,"top":690.0})),
            },
            crate::OcrWord {
                text: "24%".into(),
                confidence: None,
                bounding_box: Some(json!({"left":112.0,"bottom":680.0,"right":184.0,"top":690.0})),
            },
        ];
        let mut response = search_response("Revenue 24%", 50, Vec::new());
        fuse_search_ocr_outcomes(
            &mut response,
            vec![outcome(ocr_page("ignored", Some(words)))],
        )
        .expect("fuse OCR search");

        let item = &response.results[0]["matches"][0];
        assert_eq!(item["id"], json!("p1-ocr-match-1"));
        assert_eq!(item["match_start"], json!(13));
        assert_eq!(item["match_end"], json!(24));
        assert_eq!(item["snippet"], json!("...e Revenue 24%"));
        assert_eq!(item["ocr_word_index"], json!(2));
        assert_eq!(
            item["bounding_box"],
            json!({"left":40.0,"bottom":680.0,"right":184.0,"top":690.0})
        );
        assert_eq!(item["bounding_box_level"], json!("ocr_word"));
        assert!(item.get("text_item_index").is_none());
    }

    #[test]
    fn ocr_text_fallback_has_no_word_geometry() {
        let mut response = search_response("rocket", 50, Vec::new());
        response.search_options["context_chars"] = json!(1);
        fuse_search_ocr_outcomes(
            &mut response,
            vec![outcome(ocr_page("Astral 🚀 rocket", None))],
        )
        .expect("fuse text-only OCR");
        let item = &response.results[0]["matches"][0];
        assert_eq!(item["match_start"], json!(10));
        assert_eq!(item["match_end"], json!(16));
        assert!(item.get("ocr_word_index").is_none());
        assert!(item.get("bounding_box").is_none());
        assert!(item.get("bounding_box_level").is_none());
    }

    #[test]
    fn prefer_speed_strips_geometry_and_emits_route_warning() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential/v3014-behavior-v1.pdf");
        let response = search_pdf(&SearchPdfInput {
            sources: vec![SearchPdfSource {
                path: Some(fixture.to_string_lossy().into_owned()),
                url: None,
                pages: None,
            }],
            query: "Needle".into(),
            case_sensitive: None,
            whole_word: None,
            include_ocr_text_layer: None,
            max_pages: Some(10),
            max_matches_per_source: Some(20),
            context_chars: Some(4),
            prefer_speed: Some(true),
        })
        .expect("prefer_speed search");
        assert_eq!(response.search_options["prefer_speed"], json!(true));
        let result = &response.results[0];
        assert_eq!(result["success"], json!(true));
        let warnings = result["warnings"].as_array().expect("warnings");
        assert!(
            warnings.iter().any(|w| w
                .as_str()
                .is_some_and(|s| { s.contains("Bounding boxes are unavailable on this route") })),
            "missing speed-route warning: {warnings:?}"
        );
        let matches = result["matches"].as_array().expect("matches");
        assert!(!matches.is_empty());
        for item in matches {
            assert!(item.get("bounding_box").is_none(), "{item}");
            assert!(item.get("bounding_box_level").is_none(), "{item}");
            assert!(item.get("text_item_index").is_none(), "{item}");
            assert_eq!(item["provenance"]["engine"], json!("rust-text-index"));
            assert_eq!(item["provenance"]["source"], json!("text-content"));
        }
    }

    #[test]
    fn ocr_appends_after_selectable_and_only_cap_plus_one_truncates() {
        let selectable = json!({"id":"p1-match-1","page":1,"text":"x"});
        let mut exact = search_response("x", 2, vec![selectable.clone()]);
        fuse_search_ocr_outcomes(&mut exact, vec![outcome(ocr_page("x", None))])
            .expect("exact cap");
        assert_eq!(
            exact.results[0]["matches"][1]["id"],
            json!("p1-ocr-match-2")
        );
        assert!(exact.results[0].get("truncated").is_none());

        let mut overflow = search_response("x", 2, vec![selectable]);
        fuse_search_ocr_outcomes(&mut overflow, vec![outcome(ocr_page("x x", None))])
            .expect("cap plus one");
        assert_eq!(overflow.results[0]["matches"].as_array().unwrap().len(), 2);
        assert_eq!(overflow.results[0]["truncated"], json!(true));
        assert_eq!(
            overflow.results[0]["warnings"],
            json!(["Search results truncated to 2 matches for this source."])
        );
    }

    #[test]
    fn ocr_failure_is_nonfatal_and_preserves_warning_order() {
        let mut response = search_response("x", 2, Vec::new());
        response.results[0]["warnings"] = json!(["earlier warning"]);
        fuse_search_ocr_outcomes(
            &mut response,
            vec![SourceOcrOutcome {
                source_index: 0,
                pages: Vec::new(),
                warnings: vec!["renderer warning".into()],
                error: Some("provider failed".into()),
            }],
        )
        .expect("soft provider failure");
        assert_eq!(response.results[0]["success"], json!(true));
        assert_eq!(
            response.results[0]["warnings"],
            json!([
                "earlier warning",
                "renderer warning",
                "OCR search unavailable: provider failed"
            ])
        );
    }

    #[test]
    fn ocr_search_budget_accepts_exact_caps_and_sticks_after_plus_one() {
        let mut word_budget = OcrSearchBudget::default();
        word_budget
            .charge(MAX_OCR_SEARCH_WORDS, 0)
            .expect("exact word cap");
        let error = word_budget.charge(1, 0).expect_err("word cap plus one");
        assert_eq!(
            error,
            "Request exceeds OCR search work limit of 250000 words."
        );
        assert_eq!(word_budget.charge(0, 0), Err(error));

        let mut text_budget = OcrSearchBudget::default();
        text_budget
            .charge(0, MAX_OCR_SEARCH_UTF16_UNITS)
            .expect("exact text cap");
        let error = text_budget.charge(0, 1).expect_err("text cap plus one");
        assert_eq!(
            error,
            "Request exceeds OCR search work limit of 2000000 UTF-16 units."
        );
        assert_eq!(text_budget.charge(0, 0), Err(error));
    }
}
