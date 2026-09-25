use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::document_twin::PageText;

const ISSUE_TYPES: [&str; 11] = [
    "mark_info_missing",
    "untagged_pdf",
    "suspect_tags",
    "structure_tree_missing",
    "untagged_page",
    "heading_structure",
    "tagged_content_mismatch",
    "image_alt_text",
    "form_field_label",
    "link_label",
    "accessibility_permission",
];

#[derive(Clone, Copy, Default)]
struct RoleStats {
    roles: usize,
    contents: usize,
    content_ids: usize,
    headings: usize,
    figures: usize,
}

impl RoleStats {
    fn add(&mut self, other: Self) {
        self.roles += other.roles;
        self.contents += other.contents;
        self.content_ids += other.content_ids;
        self.headings += other.headings;
        self.figures += other.figures;
    }
}

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
        .cloned()
        .unwrap_or_default();
    let marked = mark_bool(input.mark_info, "Marked");
    let suspects = mark_bool(input.mark_info, "Suspects");
    let mut document_issues = Vec::new();
    if marked.is_none() && trees.is_empty() {
        document_issues.push(issue(
            "mark_info_missing",
            "medium",
            None,
            "PDF mark info and tagged structure trees were not exposed; accessibility tagging cannot be verified.",
            None,
        ));
    } else if marked == Some(false) {
        document_issues.push(issue(
            "untagged_pdf",
            "high",
            None,
            "PDF mark info reports that the document is not tagged.",
            input.mark_info.map(|value| json!({"mark_info":value})),
        ));
    }
    if suspects == Some(true) {
        document_issues.push(issue(
            "suspect_tags",
            "high",
            None,
            "PDF mark info reports suspect tags; verify structure before relying on semantics.",
            input.mark_info.map(|value| json!({"mark_info":value})),
        ));
    }
    if trees.is_empty() {
        document_issues.push(issue(
            "structure_tree_missing",
            "medium",
            None,
            "No tagged PDF structure tree was found for the selected pages, so heading, list, table, and figure semantics are not machine-verifiable.",
            None,
        ));
    }
    if input.permissions.is_some_and(|value| {
        value.as_array().is_some_and(|permissions| {
            !permissions.is_empty()
                && !permissions
                    .iter()
                    .any(|permission| permission.as_str() == Some("copy_for_accessibility"))
        })
    }) {
        document_issues.push(issue(
            "accessibility_permission",
            "high",
            None,
            "PDF permissions do not expose copy_for_accessibility.",
            input.permissions.map(|value| json!({"permissions":value})),
        ));
    }

    let outline_count = count_outline(input.outline);
    let mut page_reports = Vec::new();
    for page in &selected_pages {
        let tree = trees
            .iter()
            .find(|entry| entry.get("page").and_then(Value::as_u64) == Some(u64::from(*page)));
        let stats = tree
            .and_then(|entry| entry.get("tree"))
            .map(count_roles)
            .unwrap_or_default();
        let page_elements = input
            .elements
            .as_array()
            .into_iter()
            .flatten()
            .filter(|element| element.get("page").and_then(Value::as_u64) == Some(u64::from(*page)))
            .collect::<Vec<_>>();
        let visible = page_elements.len();
        let coverage = if tree.is_none() {
            0.0
        } else if visible == 0 {
            1.0
        } else {
            round_ratio((stats.contents as f64 / visible as f64).min(1.0))
        };
        let images = page_elements
            .iter()
            .filter(|element| element.get("type").and_then(Value::as_str) == Some("image"))
            .count();
        let annotations = page_annotations(input.annotations, *page);
        let links = annotations
            .iter()
            .filter(|annotation| annotation.get("url").is_some())
            .count();
        let fields = page_fields(input.form_fields, *page);
        let mut issues = Vec::new();
        if tree.is_none() {
            issues.push(issue(
                "untagged_page",
                "medium",
                Some(*page),
                "Selected page does not expose a tagged structure tree.",
                None,
            ));
        }
        if tree.is_some() && stats.headings == 0 && outline_count > 0 {
            issues.push(issue(
                "heading_structure",
                "low",
                Some(*page),
                "The document has outline entries, but this page does not expose heading roles in the structure tree.",
                Some(json!({"outline_count":outline_count})),
            ));
        }
        if tree.is_some() && visible > 0 && coverage < 0.5 {
            issues.push(issue(
                "tagged_content_mismatch",
                "medium",
                Some(*page),
                "Tagged structure exposes too few content references for the visible page content; tag-to-content coverage needs verification.",
                Some(json!({
                    "visible_element_count":visible,
                    "structure_content_count":stats.contents,
                    "structure_content_id_count":stats.content_ids,
                    "tag_content_coverage":coverage,
                })),
            ));
        }
        if images > 0 && stats.figures < images {
            issues.push(issue(
                "image_alt_text",
                if tree.is_some() { "medium" } else { "high" },
                Some(*page),
                "Page image objects outnumber Figure roles; image alt-text coverage cannot be verified from the available PDF structure.",
                Some(json!({"image_count":images,"figure_role_count":stats.figures})),
            ));
        }
        for field in &fields {
            let name = field
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let generated = is_generated_field_name(name);
            if name.is_empty() || generated {
                issues.push(issue(
                    "form_field_label",
                    if field.get("required").and_then(Value::as_bool) == Some(true) {
                        "medium"
                    } else {
                        "low"
                    },
                    Some(*page),
                    "Form field does not expose a useful accessible name.",
                    Some(json!({
                        "field_id":field.get("id"),"field_name":field.get("name"),
                        "required":field.get("required"),"type":field.get("type")
                    })),
                ));
            }
        }
        for annotation in &annotations {
            if annotation.get("url").is_some()
                && annotation.get("contents").is_none()
                && annotation.get("title").is_none()
            {
                issues.push(issue(
                    "link_label",
                    "low",
                    Some(*page),
                    "Link annotation target is present, but an accessible label was not exposed.",
                    Some(json!({
                        "annotation_id":annotation.get("id"),"subtype":annotation.get("subtype"),
                        "url":annotation.get("url")
                    })),
                ));
            }
        }
        let severity = severity_counts(&issues);
        let score = score(&issues);
        page_reports.push(json!({
            "page":page,"tagged":stats.roles>0,"score":score,"grade":grade(score),
            "structure_role_count":stats.roles,"structure_content_count":stats.contents,
            "structure_content_id_count":stats.content_ids,"visible_element_count":visible,
            "tag_content_coverage":coverage,"heading_count":stats.headings,"figure_count":stats.figures,
            "image_count":images,"link_count":links,"form_field_count":fields.len(),
            "issue_count":issues.len(),"high_issue_count":severity["high"],
            "medium_issue_count":severity["medium"],"low_issue_count":severity["low"],
            "issue_type_counts":type_counts(&issues),"issues":issues,
        }));
    }
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
        "grade":grade(total_score),"tagged":input.structure_valid && (marked==Some(true)||tagged_pages>0),
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

fn is_generated_field_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("unnamed")
        || lower.strip_prefix("field").is_some_and(|rest| {
            !rest.is_empty() && rest.chars().all(|character| character.is_ascii_digit())
        })
}

fn count_roles(node: &Value) -> RoleStats {
    let role = node
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let mut stats = RoleStats {
        roles: 1,
        headings: usize::from(
            matches!(role.as_str(), "h" | "heading")
                || role.len() == 2
                    && role.starts_with('h')
                    && role.as_bytes()[1].is_ascii_digit()
                    && (b'1'..=b'6').contains(&role.as_bytes()[1]),
        ),
        figures: usize::from(role == "figure"),
        ..RoleStats::default()
    };
    for child in node
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if child.get("role").is_some() {
            stats.add(count_roles(child));
        } else {
            stats.contents += 1;
            stats.content_ids += usize::from(child.get("id").is_some());
        }
    }
    stats
}

fn issue(
    kind: &str,
    severity: &str,
    page: Option<u32>,
    message: &str,
    evidence: Option<Value>,
) -> Value {
    let mut value = json!({"type":kind,"severity":severity,"message":message});
    if let Some(page) = page {
        value["page"] = json!(page);
    }
    if let Some(evidence) = evidence {
        value["evidence"] = evidence;
    }
    value
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
fn round_ratio(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
fn mark_bool(value: Option<&Value>, key: &str) -> Option<bool> {
    value
        .and_then(|v| v.get(key).or_else(|| v.get(key.to_ascii_lowercase())))
        .and_then(Value::as_bool)
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
fn count_outline(value: Option<&Value>) -> usize {
    fn walk(items: &[Value]) -> usize {
        items
            .iter()
            .map(|i| {
                1 + i
                    .get("items")
                    .and_then(Value::as_array)
                    .map_or(0, |a| walk(a))
            })
            .sum()
    }
    value.and_then(Value::as_array).map_or(0, |a| walk(a))
}
fn page_annotations(value: Option<&Value>, page: u32) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|g| g.get("page").and_then(Value::as_u64) == Some(u64::from(page)))
        .and_then(|g| g.get("annotations"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}
fn page_fields(value: Option<&Value>, page: u32) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| f.get("page").and_then(Value::as_u64) == Some(u64::from(page)))
        .cloned()
        .collect()
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

#[cfg(test)]
mod tests {
    use super::is_generated_field_name;

    #[test]
    fn generated_field_names_match_the_case_insensitive_ts_regex() {
        for generated in ["unnamed", "Unnamed widget", "field1", "Field123", "FIELD0"] {
            assert!(is_generated_field_name(generated), "{generated}");
        }
        for useful in ["field", "Field12x", "xfield12", "name", ""] {
            assert!(!is_generated_field_name(useful), "{useful}");
        }
    }
}
