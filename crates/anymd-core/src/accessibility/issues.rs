use serde_json::{json, Value};

use super::signals::PageSignals;

pub(super) const ISSUE_TYPES: [&str; 11] = [
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

pub(super) fn document_issues(
    marked: Option<bool>,
    suspects: Option<bool>,
    trees_missing: bool,
    mark_info: Option<&Value>,
    permissions: Option<&Value>,
) -> Vec<Value> {
    let mut document_issues = Vec::new();
    if marked.is_none() && trees_missing {
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
            mark_info.map(|value| json!({"mark_info":value})),
        ));
    }
    if suspects == Some(true) {
        document_issues.push(issue(
            "suspect_tags",
            "high",
            None,
            "PDF mark info reports suspect tags; verify structure before relying on semantics.",
            mark_info.map(|value| json!({"mark_info":value})),
        ));
    }
    if trees_missing {
        document_issues.push(issue(
            "structure_tree_missing",
            "medium",
            None,
            "No tagged PDF structure tree was found for the selected pages, so heading, list, table, and figure semantics are not machine-verifiable.",
            None,
        ));
    }
    if permissions.is_some_and(|value| {
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
            permissions.map(|value| json!({"permissions":value})),
        ));
    }
    document_issues
}

pub(super) fn page_issues(signals: &PageSignals, outline_count: usize) -> Vec<Value> {
    let PageSignals {
        page,
        has_tree,
        stats,
        visible,
        coverage,
        images,
        ref fields,
        ref annotations,
        ..
    } = *signals;
    let mut issues = Vec::new();
    if !has_tree {
        issues.push(issue(
            "untagged_page",
            "medium",
            Some(page),
            "Selected page does not expose a tagged structure tree.",
            None,
        ));
    }
    if has_tree && stats.headings == 0 && outline_count > 0 {
        issues.push(issue(
            "heading_structure",
            "low",
            Some(page),
            "The document has outline entries, but this page does not expose heading roles in the structure tree.",
            Some(json!({"outline_count":outline_count})),
        ));
    }
    if has_tree && visible > 0 && coverage < 0.5 {
        issues.push(issue(
            "tagged_content_mismatch",
            "medium",
            Some(page),
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
            if has_tree { "medium" } else { "high" },
            Some(page),
            "Page image objects outnumber Figure roles; image alt-text coverage cannot be verified from the available PDF structure.",
            Some(json!({"image_count":images,"figure_role_count":stats.figures})),
        ));
    }
    for field in fields {
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
                Some(page),
                "Form field does not expose a useful accessible name.",
                Some(json!({
                    "field_id":field.get("id"),"field_name":field.get("name"),
                    "required":field.get("required"),"type":field.get("type")
                })),
            ));
        }
    }
    for annotation in annotations {
        if annotation.get("url").is_some()
            && annotation.get("contents").is_none()
            && annotation.get("title").is_none()
        {
            issues.push(issue(
                "link_label",
                "low",
                Some(page),
                "Link annotation target is present, but an accessible label was not exposed.",
                Some(json!({
                    "annotation_id":annotation.get("id"),"subtype":annotation.get("subtype"),
                    "url":annotation.get("url")
                })),
            ));
        }
    }
    issues
}

fn is_generated_field_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("unnamed")
        || lower.strip_prefix("field").is_some_and(|rest| {
            !rest.is_empty() && rest.chars().all(|character| character.is_ascii_digit())
        })
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
