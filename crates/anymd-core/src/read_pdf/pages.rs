//! Page selection for read_pdf: page specs, even sampling and filtering extracted pages.

use super::*;

pub(super) fn join_page_text(pages: &[crate::document_twin::PageText]) -> String {
    pages
        .iter()
        .map(|page| page.text.as_str())
        .filter(|page| !page.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn parse_page_spec(pages_spec: &Option<Value>) -> Result<Option<Vec<u32>>, ReadPdfError> {
    let Some(spec) = pages_spec else {
        return Ok(None);
    };
    let mut wanted = Vec::new();
    if let Some(arr) = spec.as_array() {
        if arr.is_empty() {
            return Err(ReadPdfError::invalid_params(
                "Page specification resulted in an empty set of pages.",
            ));
        }
        for value in arr {
            let page = value
                .as_u64()
                .filter(|page| *page > 0 && *page <= u64::from(u32::MAX));
            let Some(page) = page else {
                return Err(ReadPdfError::invalid_params(
                    "Page numbers in array must be positive integers.",
                ));
            };
            wanted.push(page as u32);
            if wanted.len() > MAX_SELECTED_PAGES {
                return Err(ReadPdfError::invalid_params(
                    "Page specification exceeds the maximum of 10001 selected pages.",
                ));
            }
        }
    } else if let Some(ranges) = spec.as_str() {
        if ranges.is_empty() {
            return Err(ReadPdfError::invalid_params("Invalid page number: "));
        }
        for raw_part in ranges.split(',') {
            let part = raw_part.trim();
            if let Some((start_text, end_text)) = part.split_once('-') {
                let start = parse_ts_positive_page(start_text);
                let end = if end_text.trim().is_empty() {
                    start.map(|page| page.saturating_add(10_000))
                } else {
                    parse_ts_positive_page(end_text)
                };
                let (Some(start), Some(end)) = (start, end) else {
                    return Err(ReadPdfError::invalid_params(format!(
                        "Invalid page range values: {part}"
                    )));
                };
                if start > end {
                    return Err(ReadPdfError::invalid_params(format!(
                        "Invalid page range values: {part}"
                    )));
                }
                wanted.extend(start..=end.min(start.saturating_add(10_000)));
            } else {
                let Some(page) = parse_ts_positive_page(part) else {
                    return Err(ReadPdfError::invalid_params(format!(
                        "Invalid page number: {part}"
                    )));
                };
                wanted.push(page);
            }
            if wanted.len() > MAX_SELECTED_PAGES {
                return Err(ReadPdfError::invalid_params(
                    "Page specification exceeds the maximum of 10001 selected pages.",
                ));
            }
        }
    } else {
        return Err(ReadPdfError::invalid_params(
            "Page specification must be a non-empty range string or array of positive integers.",
        ));
    }
    wanted.sort_unstable();
    wanted.dedup();
    if wanted.is_empty() {
        return Err(ReadPdfError::invalid_params(
            "Page specification resulted in an empty set of pages.",
        ));
    }
    Ok(Some(wanted))
}

pub(super) fn parse_ts_positive_page(value: &str) -> Option<u32> {
    let value = value.trim_start();
    let value = value.strip_prefix('+').unwrap_or(value);
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty())
        .then(|| digits.parse::<u32>().ok())
        .flatten()
        .filter(|page| *page > 0)
}

pub(super) fn evenly_sample_pages(total_pages: u32, max_samples: u32) -> Vec<u32> {
    let max_samples = max_samples.clamp(1, 20).min(total_pages.max(1));
    if total_pages <= max_samples {
        return (1..=total_pages).collect();
    }
    if max_samples == 1 {
        return vec![1];
    }
    let mut selected = Vec::with_capacity(max_samples as usize);
    for index in 0..max_samples {
        let numerator = u64::from(index) * u64::from(total_pages - 1);
        let denominator = u64::from(max_samples - 1);
        // Math.round for non-negative values.
        let offset = (numerator + denominator / 2) / denominator;
        selected.push(1 + offset as u32);
    }
    selected.sort_unstable();
    selected.dedup();
    selected
}

pub(super) fn select_pages(
    pages: &[crate::text_index::ExtractedPageText],
    requested_pages: Option<&[u32]>,
) -> (Vec<crate::document_twin::PageText>, Vec<u32>) {
    let all: Vec<crate::document_twin::PageText> = pages
        .iter()
        .enumerate()
        .map(|(i, extracted)| crate::document_twin::PageText {
            page: (i + 1) as u32,
            text: extracted.text.clone(),
            positioned_items: extracted.positioned_items.clone(),
        })
        .collect();
    let Some(wanted) = requested_pages else {
        return (all, Vec::new());
    };
    let total_pages = pages.len() as u32;
    let invalid = wanted
        .iter()
        .copied()
        .filter(|page| *page > total_pages)
        .collect();
    let selected = all
        .into_iter()
        .filter(|page| wanted.binary_search(&page.page).is_ok())
        .collect();
    (selected, invalid)
}
