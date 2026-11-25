//! Trust report: evidence redaction, signal scoring, and guidance.

mod redaction;
mod report;
mod scoring;
mod signals;

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use serde_json::Value;

use super::PageText;

pub fn build_trust_report(
    pages: &[PageText],
    safety: &Value,
    layout: &Value,
    elements: &Value,
    annotations: Option<&Value>,
    redaction_policy: &str,
) -> Value {
    let mut selected_pages = pages.iter().map(|page| page.page).collect::<Vec<_>>();
    selected_pages.sort_unstable();
    selected_pages.dedup();
    let selected = selected_pages.iter().copied().collect::<HashSet<_>>();
    let collected = signals::collect_signals(
        &selected,
        safety,
        layout,
        elements,
        annotations,
        redaction_policy,
    );
    report::assemble_report(selected_pages, collected, redaction_policy)
}
