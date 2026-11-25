use serde_json::{json, Value};

use crate::{text_index::TextSearchResult, ENGINE_NAME, ENGINE_VERSION};

pub(super) fn failed_source(label: &str, message: String) -> Value {
    json!({
        "source": label,
        "success": false,
        "error": message,
    })
}

pub(super) fn format_result(
    search: TextSearchResult,
    label: &str,
    page_filter: Option<Vec<u32>>,
    max_pages: u32,
    max_matches: u32,
    speed_route: bool,
) -> Value {
    let requested_pages = page_filter.unwrap_or_else(|| (1..=search.num_pages).collect());
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
        return failed_source(
            label,
            format!("No valid pages to search for source {label}."),
        );
    }
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
    result
}
