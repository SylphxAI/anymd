use serde_json::{json, Value};

use crate::{text_index::search_literal_text_bounded, SourceOcrOutcome};

use super::{SearchPdfError, SearchPdfResponse, DEFAULT_CONTEXT_CHARS, DEFAULT_MAX_MATCHES};

pub(super) const MAX_OCR_SEARCH_WORDS: usize = 250_000;
pub(super) const MAX_OCR_SEARCH_UTF16_UNITS: usize = 2_000_000;

#[derive(Default)]
pub(super) struct OcrSearchBudget {
    words: usize,
    utf16_units: usize,
    exhausted: Option<String>,
}

impl OcrSearchBudget {
    fn exhaust(&mut self, message: String) -> Result<(), String> {
        self.exhausted = Some(message.clone());
        Err(message)
    }

    pub(super) fn charge(&mut self, words: usize, utf16_units: usize) -> Result<(), String> {
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
