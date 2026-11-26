//! PDF-specific read routing, Markdown assembly, and scan notices.

use rmcp::model::CallToolResult;

use crate::document::{OpenOptions, Opened};
use crate::page_selection::selected_pages;
use crate::schema::{PdfSource, ReadPdfArgs};
use crate::source_access::SourceAccessPolicy;

use super::super::{estimate_tokens, finish};
use super::front_matter;
use super::pagination::{budget_from, continuation_note, failed, read_opened, Cursor, SourceRead};

/// True when the caller asked for the evidence-heavy JSON read instead.
pub fn read_wants_legacy(args: &ReadPdfArgs) -> bool {
    let profile_is_markdown = args
        .profile
        .as_deref()
        .is_some_and(|profile| profile.eq_ignore_ascii_case("markdown"));
    (args.profile.is_some() && !profile_is_markdown)
        || args.auto.is_some()
        || args.auto_detail.is_some()
        || args.sample_pages.is_some()
        || args.max_visual_enrichments.is_some()
        || args.trust_report_redaction.is_some()
        || [
            args.include_full_text,
            args.include_metadata,
            args.include_page_count,
            args.include_images,
            args.include_tables,
            args.include_elements,
            args.include_semantic_hints,
            args.include_markdown,
            args.include_html,
            args.include_chunks,
            args.include_text_layer,
            args.include_ocr_text_layer,
            args.include_outline,
            args.include_annotations,
            args.include_page_labels,
            args.include_page_geometry,
            args.include_permissions,
            args.include_form_fields,
            args.include_attachments,
            args.include_structure_tree,
            args.include_safety_findings,
            args.include_layout_diagnostics,
            args.include_document_map,
            args.include_document_ast,
            args.include_visual_enrichments,
            args.include_trust_report,
            args.include_accessibility_report,
        ]
        .iter()
        .any(Option::is_some)
}

/// Legacy `read_pdf` default route: the same Markdown answer for PDF sources.
pub fn read_pdf(
    args: &ReadPdfArgs,
    policy: &SourceAccessPolicy,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let budget = budget_from(args.max_tokens);
    let cursor = args
        .cursor
        .as_deref()
        .map(Cursor::parse)
        .transpose()
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    if cursor.is_some() && args.sources.len() != 1 {
        return Err(rmcp::ErrorData::invalid_params(
            "cursor continues a single-source read; pass exactly one source with it.",
            None,
        ));
    }
    let mut out = String::new();
    let mut failures = 0usize;
    let mut skipped = Vec::new();
    for (index, source) in args.sources.iter().enumerate() {
        let remaining = budget.saturating_sub(estimate_tokens(&out));
        if index > 0 && remaining < 500 {
            skipped.push(source.label());
            continue;
        }
        let read = pdf_source_read(source, policy, cursor, remaining.max(500));
        if read.error.is_some() {
            failures += 1;
        }
        out.push_str(&front_matter(&read.header));
        out.push_str(&read.body);
        if let Some(next) = read.next {
            out.push_str(&continuation_note(budget, next));
        }
    }
    if !skipped.is_empty() {
        out.push_str(&format!(
            "<!-- Budget used up before these sources; read them separately: {} -->\n",
            skipped.join(", ")
        ));
    }
    Ok(finish(out, failures == args.sources.len()))
}

fn pdf_source_read(
    source: &PdfSource,
    policy: &SourceAccessPolicy,
    cursor: Option<Cursor>,
    budget: usize,
) -> SourceRead {
    let label = source.label();
    let selection = match selected_pages(&source.pages) {
        Ok(selection) => selection,
        Err(message) => return failed(&label, message),
    };
    match Opened::open(&label, policy, &OpenOptions::default()) {
        Ok(mut opened) => read_opened(
            &mut opened,
            selection,
            cursor,
            budget,
            cursor.is_none(),
            None,
        ),
        Err(message) => failed(&label, message),
    }
}

/// Marker for a page with no text layer that images cover (a scan nobody read).
pub(crate) fn scan_marker(page: u32, ocr_ran: bool) -> String {
    if ocr_ran {
        return format!("<!-- page {page}: scanned image; OCR found no text -->\n\n");
    }
    format!(
        "<!-- page {page}: scanned image, no text layer; to enable local OCR, run `anymd setup ocr` (opts into downloading the OCR engine and ~2 GB of model weights), then retry with OCR enabled (`ocr: true` in MCP or `--ocr auto` in the CLI); documents stay on this machine -->\n\n"
    )
}
