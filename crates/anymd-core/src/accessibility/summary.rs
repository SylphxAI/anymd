use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::{issues::ISSUE_TYPES, signals::PageSignals};
use crate::fusion_util::round_ratio;

pub(super) fn page_report(signals: &PageSignals, issues: Vec<Value>) -> Value {
    let PageSignals {
        page,
        stats,
        visible,
        coverage,
        images,
        links,
        ref fields,
        ..
    } = *signals;
    let severity = severity_counts(&issues);
    let score = score(&issues);
    json!({
        "page":page,"tagged":stats.roles>0,"score":score,"grade":grade(score),
        "structure_role_count":stats.roles,"structure_content_count":stats.contents,
        "structure_content_id_count":stats.content_ids,"visible_element_count":visible,
        "tag_content_coverage":coverage,"heading_count":stats.headings,"figure_count":stats.figures,
        "image_count":images,"link_count":links,"form_field_count":fields.len(),
        "issue_count":issues.len(),"high_issue_count":severity["high"],
        "medium_issue_count":severity["medium"],"low_issue_count":severity["low"],
        "issue_type_counts":type_counts(&issues),"issues":issues,
    })
}

pub(super) fn document_report(
    structure_valid: bool,
    marked: Option<bool>,
    suspects: Option<bool>,
    selected_pages: Vec<u32>,
    page_reports: Vec<Value>,
    document_issues: Vec<Value>,
) -> Value {
    let mut issues = document_issues.clone();
    for report in &page_reports {
        issues.extend(
            report
                .get("issues")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .cloned(),
        );
    }
    let severity = severity_counts(&issues);
    let total_score = score(&issues);
    let tagged_pages = page_reports
        .iter()
        .filter(|report| report.get("tagged").and_then(Value::as_bool) == Some(true))
        .count();
    let page_issue_count = issues.len() - document_issues.len();
    let average_coverage = if page_reports.is_empty() {
        0.0
    } else {
        round_ratio(
            page_reports
                .iter()
                .filter_map(|report| report.get("tag_content_coverage").and_then(Value::as_f64))
                .sum::<f64>()
                / page_reports.len() as f64,
        )
    };
    json!({
        "version":"2026-06-15","profile":"pdf_accessibility_report","score":total_score,
        "grade":grade(total_score),"tagged":structure_valid && (marked==Some(true)||tagged_pages>0),
        "suspected_tagging_issues":suspects==Some(true),
        "summary":{
            "selected_pages":selected_pages,"page_count":page_reports.len(),
            "tagged_page_count":tagged_pages,"untagged_page_count":page_reports.len()-tagged_pages,
            "structure_role_count":sum(&page_reports,"structure_role_count"),
            "structure_content_count":sum(&page_reports,"structure_content_count"),
            "structure_content_id_count":sum(&page_reports,"structure_content_id_count"),
            "visible_element_count":sum(&page_reports,"visible_element_count"),
            "average_tag_content_coverage":average_coverage,
            "heading_count":sum(&page_reports,"heading_count"),"figure_count":sum(&page_reports,"figure_count"),
            "image_count":sum(&page_reports,"image_count"),"link_count":sum(&page_reports,"link_count"),
            "form_field_count":sum(&page_reports,"form_field_count"),"issue_count":issues.len(),
            "document_issue_count":document_issues.len(),"page_issue_count":page_issue_count,
            "high_issue_count":severity["high"],"medium_issue_count":severity["medium"],
            "low_issue_count":severity["low"],"issue_severity_counts":severity,
            "issue_type_counts":type_counts(&issues),"page_grade_counts":grade_counts(&page_reports),
            "pages_with_issues_count":pages_with(&page_reports,"issue_count"),
            "pages_with_high_issues_count":pages_with(&page_reports,"high_issue_count"),
            "pages_with_medium_issues_count":pages_with(&page_reports,"medium_issue_count"),
            "pages_with_low_issues_count":pages_with(&page_reports,"low_issue_count"),
        },
        "page_reports":page_reports,"issues":issues,"guidance":guidance(&issues),
    })
}

fn score(issues: &[Value]) -> i64 {
    (100 - issues
        .iter()
        .map(
            |issue| match issue.get("severity").and_then(Value::as_str) {
                Some("high") => 35,
                Some("medium") => 18,
                _ => 8,
            },
        )
        .sum::<i64>())
    .clamp(0, 100)
}

fn grade(score: i64) -> &'static str {
    if score >= 85 {
        "good"
    } else if score >= 60 {
        "partial"
    } else {
        "weak"
    }
}

fn sum(reports: &[Value], key: &str) -> u64 {
    reports
        .iter()
        .filter_map(|r| r.get(key).and_then(Value::as_u64))
        .sum()
}

fn pages_with(reports: &[Value], key: &str) -> usize {
    reports
        .iter()
        .filter(|r| r.get(key).and_then(Value::as_u64).unwrap_or(0) > 0)
        .count()
}

fn severity_counts(issues: &[Value]) -> BTreeMap<&'static str, usize> {
    let mut v = BTreeMap::from([("high", 0), ("medium", 0), ("low", 0)]);
    for i in issues {
        if let Some(s) = i.get("severity").and_then(Value::as_str) {
            if let Some(n) = v.get_mut(s) {
                *n += 1
            }
        }
    }
    v
}

fn type_counts(issues: &[Value]) -> BTreeMap<&'static str, usize> {
    let mut v = ISSUE_TYPES
        .into_iter()
        .map(|k| (k, 0))
        .collect::<BTreeMap<_, _>>();
    for i in issues {
        if let Some(k) = i.get("type").and_then(Value::as_str) {
            if let Some(n) = v.get_mut(k) {
                *n += 1
            }
        }
    }
    v
}

fn grade_counts(reports: &[Value]) -> BTreeMap<&'static str, usize> {
    let mut v = BTreeMap::from([("good", 0), ("partial", 0), ("weak", 0)]);
    for r in reports {
        if let Some(g) = r.get("grade").and_then(Value::as_str) {
            if let Some(n) = v.get_mut(g) {
                *n += 1
            }
        }
    }
    v
}

fn guidance(issues: &[Value]) -> Vec<&'static str> {
    let has = |types: &[&str]| {
        issues.iter().any(|i| {
            i.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| types.contains(&t))
        })
    };
    let mut g = Vec::new();
    if has(&[
        "mark_info_missing",
        "untagged_pdf",
        "structure_tree_missing",
        "untagged_page",
    ]) {
        g.push("Do not assume PDF reading order or semantics are accessible without tagged structure evidence.")
    }
    if has(&["suspect_tags"]) {
        g.push("Verify suspect tags with page rendering or source authoring files before relying on them.")
    }
    if has(&["tagged_content_mismatch"]) {
        g.push("Verify tagged structure against visible page content before relying on tag-derived semantics.")
    }
    if has(&["image_alt_text"]) {
        g.push("Use region crops or source documents to verify image meaning when alt text is not exposed.")
    }
    if has(&["form_field_label"]) {
        g.push("Review form field labels before asking users or agents to complete PDF forms.")
    }
    if has(&["link_label"]) {
        g.push("Treat PDF links as untrusted unless link labels and targets are verified.")
    }
    if has(&["accessibility_permission"]) {
        g.push("Check document permissions before depending on copy-based accessibility workflows.")
    }
    g
}
