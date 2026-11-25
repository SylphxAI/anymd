//! Assemble the document report and its per-page summaries.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::super::TRUST_REPORT_VERSION;
use super::scoring::{count_string_field, risk_from_score, severity_score, trust_guidance};
use super::signals::CollectedSignals;

pub(super) fn assemble_report(
    selected_pages: Vec<u32>,
    collected: CollectedSignals,
    redaction_policy: &str,
) -> Value {
    let CollectedSignals {
        safety_findings,
        signals,
    } = collected;
    let mut signals_by_page = BTreeMap::<u32, Vec<Value>>::new();
    for signal in &signals {
        if let Some(page) = signal
            .get("page")
            .and_then(Value::as_u64)
            .and_then(|page| u32::try_from(page).ok())
        {
            signals_by_page
                .entry(page)
                .or_default()
                .push(signal.clone());
        }
    }
    let page_reports = selected_pages
        .iter()
        .map(|page| {
            let page_signals = signals_by_page.remove(page).unwrap_or_default();
            let score = page_signals
                .iter()
                .fold(0u32, |sum, signal| {
                    sum.saturating_add(severity_score(
                        signal.get("severity").and_then(Value::as_str),
                    ))
                })
                .min(100);
            json!({"page": page, "risk": risk_from_score(score), "score": score, "signals": page_signals})
        })
        .collect::<Vec<_>>();
    let score = signals
        .iter()
        .fold(0u32, |sum, signal| {
            sum.saturating_add(severity_score(
                signal.get("severity").and_then(Value::as_str),
            ))
        })
        .min(100);
    let count_severity = |severity: &str| {
        signals
            .iter()
            .filter(|signal| signal.get("severity").and_then(Value::as_str) == Some(severity))
            .count()
    };
    let count_risk = |risk: &str| {
        page_reports
            .iter()
            .filter(|report| report.get("risk").and_then(Value::as_str) == Some(risk))
            .count()
    };
    let guidance = trust_guidance(&signals);

    json!({
        "version": TRUST_REPORT_VERSION,
        "profile": "pdf_trust_report",
        "risk": risk_from_score(score),
        "score": score,
        "summary": {
            "selected_pages": selected_pages,
            "redaction_policy": redaction_policy,
            "signal_count": signals.len(),
            "high_signal_count": count_severity("high"),
            "medium_signal_count": count_severity("medium"),
            "low_signal_count": count_severity("low"),
            "signal_type_counts": count_string_field(&signals, "type"),
            "safety_finding_type_counts": count_string_field(&safety_findings, "type"),
            "page_count": selected_pages.len(),
            "pages_with_signals": page_reports.iter().filter(|report| report.get("signals").and_then(Value::as_array).is_some_and(|values| !values.is_empty())).count(),
            "high_risk_page_count": count_risk("high"),
            "medium_risk_page_count": count_risk("medium"),
            "low_risk_page_count": count_risk("low"),
        },
        "page_reports": page_reports,
        "signals": signals,
        "guidance": guidance,
    })
}
