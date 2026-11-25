//! Trust report routing, page indexes, and summary projection.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::sources::{page_indexes, page_number};

pub(super) struct Trust<'a> {
    report: Option<&'a Value>,
    page_reports: Option<&'a Vec<Value>>,
    signals: Option<&'a Vec<Value>>,
    report_by_page: BTreeMap<u32, (usize, &'a Value)>,
    signal_indexes_by_page: BTreeMap<u32, Vec<usize>>,
}

impl<'a> Trust<'a> {
    pub(super) fn new(trust: Option<&'a Value>) -> Self {
        let trust_page_reports = trust
            .and_then(|report| report.get("page_reports"))
            .and_then(Value::as_array);
        let trust_signals = trust
            .and_then(|report| report.get("signals"))
            .and_then(Value::as_array);
        let mut trust_report_by_page = BTreeMap::<u32, (usize, &Value)>::new();
        for (index, report) in trust_page_reports.into_iter().flatten().enumerate() {
            if let Some(page) = page_number(report).and_then(|page| u32::try_from(page).ok()) {
                trust_report_by_page.insert(page, (index, report));
            }
        }
        Self {
            report: trust,
            page_reports: trust_page_reports,
            signals: trust_signals,
            report_by_page: trust_report_by_page,
            signal_indexes_by_page: page_indexes(trust_signals.into_iter().flatten()),
        }
    }

    pub(super) fn has_signals(&self, page: u32) -> bool {
        self.signal_indexes_by_page.contains_key(&page)
    }

    pub(super) fn project_page(&self, page: u32, value: &mut Value) {
        let Some((index, report)) = self.report_by_page.get(&page).copied() else {
            return;
        };
        let trust_signal_indexes = self
            .signal_indexes_by_page
            .get(&page)
            .map_or(&[][..], Vec::as_slice);
        let trust_indexes_for_severity = |severity: &str| {
            trust_signal_indexes
                .iter()
                .copied()
                .filter(|index| {
                    self.signals
                        .and_then(|signals| signals.get(*index))
                        .and_then(|signal| signal.get("severity"))
                        .and_then(Value::as_str)
                        == Some(severity)
                })
                .collect::<Vec<_>>()
        };
        let report_signals = report
            .get("signals")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = |severity: &str| {
            report_signals
                .iter()
                .filter(|signal| signal.get("severity").and_then(Value::as_str) == Some(severity))
                .count()
        };
        value["trust_report_page_index"] = json!(index);
        value["trust_signal_indexes"] = json!(trust_signal_indexes);
        value["trust_high_signal_indexes"] = json!(trust_indexes_for_severity("high"));
        value["trust_medium_signal_indexes"] = json!(trust_indexes_for_severity("medium"));
        value["trust_low_signal_indexes"] = json!(trust_indexes_for_severity("low"));
        for key in ["risk", "score"] {
            if let Some(entry) = report.get(key) {
                value[format!("trust_{key}")] = entry.clone();
            }
        }
        value["trust_signal_count"] = json!(report_signals.len());
        value["trust_high_signal_count"] = json!(count("high"));
        value["trust_medium_signal_count"] = json!(count("medium"));
        value["trust_low_signal_count"] = json!(count("low"));
    }

    pub(super) fn append_summary(&self, output: &mut Map<String, Value>) {
        let trust_summary = self
            .report
            .and_then(|report| report.get("summary"))
            .map(|summary| {
                json!({
                    "trust_report_page_count": self.page_reports.map_or(0, Vec::len),
                    "trust_risk": self.report.and_then(|report| report.get("risk")),
                    "trust_score": self.report.and_then(|report| report.get("score")),
                    "trust_signal_count": summary.get("signal_count"),
                    "trust_high_signal_count": summary.get("high_signal_count"),
                    "trust_medium_signal_count": summary.get("medium_signal_count"),
                    "trust_low_signal_count": summary.get("low_signal_count"),
                    "trust_pages_with_signals": summary.get("pages_with_signals"),
                    "trust_high_risk_page_count": summary.get("high_risk_page_count"),
                    "trust_medium_risk_page_count": summary.get("medium_risk_page_count"),
                    "trust_signal_type_counts": summary.get("signal_type_counts"),
                })
            })
            .unwrap_or_else(|| json!({}));
        if let Value::Object(fields) = trust_summary {
            output.extend(fields);
        }
    }

    pub(super) fn append_routing(&self, routing: &mut Value) {
        let trust_pages = |predicate: fn(&Value) -> bool| {
            self.page_reports
                .into_iter()
                .flatten()
                .filter(|report| predicate(report))
                .filter_map(page_number)
                .collect::<Vec<_>>()
        };
        let trust_review_pages = trust_pages(|report| {
            report
                .get("signals")
                .and_then(Value::as_array)
                .is_some_and(|signals| !signals.is_empty())
        });
        let trust_high_signal_pages = trust_pages(|report| {
            report
                .get("signals")
                .and_then(Value::as_array)
                .is_some_and(|signals| {
                    signals.iter().any(|signal| {
                        signal.get("severity").and_then(Value::as_str) == Some("high")
                    })
                })
        });
        let trust_high_risk_pages =
            trust_pages(|report| report.get("risk").and_then(Value::as_str) == Some("high"));
        let trust_medium_risk_pages =
            trust_pages(|report| report.get("risk").and_then(Value::as_str) == Some("medium"));
        routing["trust_review_pages"] = json!(trust_review_pages);
        routing["trust_high_signal_pages"] = json!(trust_high_signal_pages);
        routing["trust_high_risk_pages"] = json!(trust_high_risk_pages);
        routing["trust_medium_risk_pages"] = json!(trust_medium_risk_pages);
    }
}
