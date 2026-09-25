//! Read presets and the checks that decide which read options a JSON request set explicitly.

use super::*;

pub(super) fn json_has_explicit_read_options(input: &Value) -> bool {
    const KEYS: &[&str] = &[
        "include_full_text",
        "include_metadata",
        "include_page_count",
        "include_images",
        "include_tables",
        "include_elements",
        "include_semantic_hints",
        "include_markdown",
        "include_html",
        "include_chunks",
        "include_text_layer",
        "include_ocr_text_layer",
        "include_outline",
        "include_annotations",
        "include_page_labels",
        "include_page_geometry",
        "include_permissions",
        "include_form_fields",
        "include_attachments",
        "include_structure_tree",
        "include_safety_findings",
        "include_layout_diagnostics",
        "include_document_map",
        "include_document_ast",
        "include_visual_enrichments",
        "max_visual_enrichments",
        "include_trust_report",
        "trust_report_redaction",
        "include_accessibility_report",
    ];
    // `pages` is a filter, not a mode switch. A null pages key is how the MCP
    // server materializes "not specified"; a real page list still uses the preset.
    KEYS.iter().any(|key| input.get(*key).is_some())
}

pub(super) fn enable_absent(input: &Value, key: &str, field: &mut bool) {
    if input.get(key).is_none() {
        *field = true;
    }
}

/// Named read presets. OCR, rendering, and visual analysis are never included.
pub(super) fn apply_read_preset(parsed: &mut ReadPdfInput, input: &Value, preset: &str) {
    let structural = matches!(preset, "quality" | "research" | "full");
    let audits = matches!(preset, "balanced" | "research" | "full");
    let known = matches!(preset, "fast" | "balanced" | "quality" | "research" | "full");
    if !known {
        return;
    }
    enable_absent(input, "include_metadata", &mut parsed.include_metadata);
    enable_absent(input, "include_page_count", &mut parsed.include_page_count);
    enable_absent(
        input,
        "include_page_geometry",
        &mut parsed.include_page_geometry,
    );
    enable_absent(input, "include_document_map", &mut parsed.include_document_map);
    enable_absent(input, "include_chunks", &mut parsed.include_chunks);
    enable_absent(input, "include_markdown", &mut parsed.include_markdown);
    enable_absent(input, "include_tables", &mut parsed.include_tables);
    enable_absent(
        input,
        "include_semantic_hints",
        &mut parsed.include_semantic_hints,
    );
    enable_absent(
        input,
        "include_layout_diagnostics",
        &mut parsed.include_layout_diagnostics,
    );
    if structural {
        enable_absent(input, "include_full_text", &mut parsed.include_full_text);
        enable_absent(input, "include_html", &mut parsed.include_html);
        enable_absent(input, "include_elements", &mut parsed.include_elements);
        enable_absent(input, "include_text_layer", &mut parsed.include_text_layer);
        enable_absent(
            input,
            "include_document_ast",
            &mut parsed.include_document_ast,
        );
        enable_absent(input, "include_outline", &mut parsed.include_outline);
        enable_absent(input, "include_annotations", &mut parsed.include_annotations);
        enable_absent(input, "include_page_labels", &mut parsed.include_page_labels);
        enable_absent(input, "include_permissions", &mut parsed.include_permissions);
        enable_absent(input, "include_form_fields", &mut parsed.include_form_fields);
        enable_absent(input, "include_attachments", &mut parsed.include_attachments);
        enable_absent(
            input,
            "include_structure_tree",
            &mut parsed.include_structure_tree,
        );
    }
    if audits {
        enable_absent(
            input,
            "include_safety_findings",
            &mut parsed.include_safety_findings,
        );
        enable_absent(input, "include_trust_report", &mut parsed.include_trust_report);
        enable_absent(
            input,
            "include_accessibility_report",
            &mut parsed.include_accessibility_report,
        );
    }
}

pub(super) fn resolved_read_preset(parsed: &ReadPdfInput, legacy_auto: bool) -> String {
    // `auto_detail` is the legacy depth switch and wins over `profile`.
    if let Some(detail) = parsed.auto_detail.as_deref() {
        return detail.to_string();
    }
    if let Some(profile) = parsed.profile.as_deref() {
        return profile.to_string();
    }
    if legacy_auto {
        "balanced".to_string()
    } else {
        "fast".to_string()
    }
}
