//! Per-page projections for selected pages only.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::super::{text_layer::utf16_len, PageText};
use super::{
    accessibility::Accessibility,
    sources::{element_kind, page_number, MapSources},
    trust::Trust,
};

pub(super) fn selected(pages: &[PageText]) -> Vec<u32> {
    let mut selected_pages = pages.iter().map(|page| page.page).collect::<Vec<_>>();
    selected_pages.sort_unstable();
    selected_pages.dedup();
    selected_pages
}

pub(super) fn project_pages(
    pages: &[PageText],
    selected_pages: &[u32],
    sources: &MapSources<'_>,
    visual_indexes: &BTreeMap<u32, Vec<usize>>,
    trust: &Trust<'_>,
    accessibility: &Accessibility<'_>,
) -> Vec<Value> {
    selected_pages
        .iter()
        .filter_map(|page| pages.iter().find(|entry| entry.page == *page))
        .map(|selected_page| {
            project_page(selected_page, sources, visual_indexes, trust, accessibility)
        })
        .collect::<Vec<_>>()
}

fn project_page(
    selected_page: &PageText,
    sources: &MapSources<'_>,
    visual_indexes: &BTreeMap<u32, Vec<usize>>,
    trust: &Trust<'_>,
    accessibility: &Accessibility<'_>,
) -> Value {
    let page = selected_page.page;
    let page_elements = sources
        .elements
        .iter()
        .filter(|element| page_number(element) == Some(u64::from(page)))
        .collect::<Vec<_>>();
    // Index only admitted selected pages. Never materialize every integer in a
    // hostile chunk page span.
    let page_chunks = sources
        .chunks
        .iter()
        .filter(|chunk| {
            let start = chunk.get("page_start").and_then(Value::as_u64);
            let end = chunk.get("page_end").and_then(Value::as_u64);
            start.is_some_and(|start| start <= u64::from(page))
                && end.is_some_and(|end| end >= u64::from(page))
        })
        .collect::<Vec<_>>();
    let chunk_ids = page_chunks
        .iter()
        .filter_map(|chunk| chunk.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let page_layout = sources
        .layout
        .iter()
        .find(|entry| page_number(entry) == Some(u64::from(page)));
    let page_geometry = sources
        .geometry
        .iter()
        .find(|entry| page_number(entry) == Some(u64::from(page)));
    let page_safety_indexes = sources
        .safety
        .iter()
        .enumerate()
        .filter(|(_, finding)| page_number(finding) == Some(u64::from(page)))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let visual_candidate_indexes = visual_indexes.get(&page).cloned().unwrap_or_default();
    let visual_candidate_count = visual_candidate_indexes.len();
    let warnings = page_warnings(
        &page_elements,
        page_layout,
        !page_safety_indexes.is_empty(),
        trust.has_signals(page),
    );
    let fallback_items = selected_page
        .text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>();
    let item_text = if selected_page.positioned_items.is_empty() {
        fallback_items
    } else {
        selected_page
            .positioned_items
            .iter()
            .map(|item| item.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
    };
    let mut value = json!({
        "page": page,
        "element_ids": page_elements.iter().filter_map(|element| element.get("id").and_then(Value::as_str)).collect::<Vec<_>>(),
        "chunk_ids": chunk_ids,
        "safety_finding_indexes": page_safety_indexes,
        "visual_candidate_indexes": visual_candidate_indexes,
        "visual_enrichment_indexes": [],
        "text_chars": item_text.iter().map(|text| utf16_len(text)).sum::<u32>(),
        "text_item_count": item_text.len(),
        "image_count": page_elements.iter().filter(|element| element_kind(element) == Some("image")).count(),
        "table_count": page_elements.iter().filter(|element| element_kind(element) == Some("table")).count(),
        "visual_candidate_count": visual_candidate_count,
        "visual_enrichment_count": 0,
    });
    if let Some(geometry) = page_geometry {
        value["geometry"] = geometry.clone();
    }
    if let Some(layout) = page_layout {
        value["layout"] = layout.clone();
    }
    sources.text.project_page(page, &mut value);
    trust.project_page(page, &mut value);
    accessibility.project_page(page, &mut value);
    if !warnings.is_empty() {
        value["warnings"] = json!(warnings);
    }
    value
}

fn page_warnings(
    page_elements: &[&Value],
    page_layout: Option<&Value>,
    has_safety_findings: bool,
    has_trust_signals: bool,
) -> Vec<Value> {
    let mut page_warnings = page_elements
        .iter()
        .filter(|element| element_kind(element) == Some("table"))
        .flat_map(|element| {
            let id = element.get("id").and_then(Value::as_str).unwrap_or("table");
            element
                .pointer("/table/quality/warnings")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(move |warning| json!(format!("{id}: {warning}")))
        })
        .collect::<Vec<_>>();
    page_warnings.extend(
        page_layout
            .and_then(|entry| entry.get("warnings"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    );
    if has_safety_findings {
        page_warnings.push(json!(
            "Page has content safety findings; inspect findings before using as instructions."
        ));
    }
    if has_trust_signals {
        page_warnings.push(json!(
            "Page has trust report signals; inspect trust evidence before using content."
        ));
    }
    page_warnings
}
