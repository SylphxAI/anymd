use serde_json::Value;

use crate::document_twin::PageText;

mod issues;
mod signals;
mod structure;
mod summary;

pub(crate) struct AccessibilityInput<'a> {
    pub pages: &'a [PageText],
    pub elements: &'a Value,
    pub structure_trees: Option<&'a Value>,
    pub annotations: Option<&'a Value>,
    pub form_fields: Option<&'a Value>,
    pub permissions: Option<&'a Value>,
    pub mark_info: Option<&'a Value>,
    pub outline: Option<&'a Value>,
    pub structure_valid: bool,
}

pub(crate) fn build_accessibility_report(input: AccessibilityInput<'_>) -> Value {
    let mut selected_pages = input.pages.iter().map(|page| page.page).collect::<Vec<_>>();
    selected_pages.sort_unstable();
    selected_pages.dedup();
    let trees = input
        .structure_trees
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let marked = structure::mark_bool(input.mark_info, "Marked");
    let suspects = structure::mark_bool(input.mark_info, "Suspects");
    let document_issues = issues::document_issues(
        marked,
        suspects,
        trees.is_empty(),
        input.mark_info,
        input.permissions,
    );
    let outline_count = structure::count_outline(input.outline);
    let page_reports = selected_pages
        .iter()
        .map(|&page| {
            let signals = signals::PageSignals::collect(&input, trees, page);
            let issues = issues::page_issues(&signals, outline_count);
            summary::page_report(&signals, issues)
        })
        .collect::<Vec<_>>();

    summary::document_report(
        input.structure_valid,
        marked,
        suspects,
        selected_pages,
        page_reports,
        document_issues,
    )
}
