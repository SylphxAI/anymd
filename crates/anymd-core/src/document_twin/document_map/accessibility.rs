//! Accessibility report routing, page indexes, and summary projection.

use serde_json::{json, Map, Value};

use super::sources::page_number;

pub(super) struct Accessibility<'a> {
    report: Option<&'a Value>,
    page_reports: Option<&'a Vec<Value>>,
    issues: Option<&'a Vec<Value>>,
}

impl<'a> Accessibility<'a> {
    pub(super) fn new(a11y: Option<&'a Value>) -> Self {
        let accessibility_page_reports = a11y
            .and_then(|report| report.get("page_reports"))
            .and_then(Value::as_array);
        let accessibility_issues = a11y
            .and_then(|report| report.get("issues"))
            .and_then(Value::as_array);
        Self {
            report: a11y,
            page_reports: accessibility_page_reports,
            issues: accessibility_issues,
        }
    }

    pub(super) fn project_page(&self, page: u32, value: &mut Value) {
        if let Some((index, report)) = self.page_reports.and_then(|reports| {
            reports
                .iter()
                .enumerate()
                .find(|(_, report)| page_number(report) == Some(u64::from(page)))
        }) {
            let issue_indexes = self
                .issues
                .into_iter()
                .flatten()
                .enumerate()
                .filter(|(_, issue)| page_number(issue) == Some(u64::from(page)))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            value["accessibility_report_page_index"] = json!(index);
            value["accessibility_issue_indexes"] = json!(issue_indexes);
            for severity in ["high", "medium", "low"] {
                let indexes = self
                    .issues
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter(|(_, issue)| {
                        page_number(issue) == Some(u64::from(page))
                            && issue.get("severity").and_then(Value::as_str) == Some(severity)
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                value[format!("accessibility_{severity}_issue_indexes")] = json!(indexes);
            }
            for key in [
                "grade",
                "score",
                "issue_count",
                "high_issue_count",
                "medium_issue_count",
                "low_issue_count",
            ] {
                if let Some(entry) = report.get(key) {
                    value[format!("accessibility_{key}")] = entry.clone();
                }
            }
        }
    }

    pub(super) fn append_summary(&self, output: &mut Map<String, Value>) {
        let accessibility_summary = self.report
            .and_then(|report| report.get("summary"))
            .map(|summary| json!({
                "accessibility_report_page_count": summary.get("page_count"),
                "accessibility_score": self.report.and_then(|report| report.get("score")),
                "accessibility_grade": self.report.and_then(|report| report.get("grade")),
                "accessibility_issue_count": summary.get("issue_count"),
                "accessibility_document_issue_count": summary.get("document_issue_count"),
                "accessibility_page_issue_count": summary.get("page_issue_count"),
                "accessibility_high_issue_count": summary.get("high_issue_count"),
                "accessibility_medium_issue_count": summary.get("medium_issue_count"),
                "accessibility_low_issue_count": summary.get("low_issue_count"),
                "accessibility_pages_with_issues_count": summary.get("pages_with_issues_count"),
                "accessibility_pages_with_high_issues_count": summary.get("pages_with_high_issues_count"),
                "accessibility_page_grade_counts": summary.get("page_grade_counts"),
            }))
            .unwrap_or_else(|| json!({}));
        if let Value::Object(fields) = accessibility_summary {
            output.extend(fields);
        }
    }

    pub(super) fn append_routing(&self, routing: &mut Value) {
        let review_pages = |severity: Option<&str>| {
            self.page_reports
                .into_iter()
                .flatten()
                .filter(|report| {
                    let key = severity
                        .map(|value| format!("{value}_issue_count"))
                        .unwrap_or_else(|| "issue_count".into());
                    report.get(key).and_then(Value::as_u64).unwrap_or(0) > 0
                })
                .filter_map(page_number)
                .collect::<Vec<_>>()
        };
        routing["accessibility_review_pages"] = json!(review_pages(None));
        routing["accessibility_high_issue_pages"] = json!(review_pages(Some("high")));
        routing["accessibility_medium_issue_pages"] = json!(review_pages(Some("medium")));
        routing["accessibility_low_issue_pages"] = json!(review_pages(Some("low")));
    }
}
