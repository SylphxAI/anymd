//! Markdown read entry points for documents and directory listings.

use std::path::Path;

use anymd_core::text_rules::yaml_value;
use ignore::WalkBuilder;
use rmcp::model::CallToolResult;

use crate::document::{is_url, readable_extension, OpenOptions, Opened};
use crate::page_selection::selected_pages;
use crate::schema::{PageSpecifier, PdfSource, ReadArgs, ReadPdfArgs};
use crate::source_access::SourceAccessPolicy;

use super::{estimate_tokens, finish};

pub(super) mod pagination;
use pagination::{budget_from, continuation_note, failed, read_opened, Cursor, SourceRead};

/// `read`: any file, URL, or directory → Markdown.
pub fn read(
    args: &ReadArgs,
    policy: &SourceAccessPolicy,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let (text, failed) = read_text(args, policy, &ReadRender::default())
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    // Missing VLM setup is guidance, not a failure: a normal result, like the
    // Pro notice, so the agent relays it.
    let setup_needed = failed && text.trim_end() == crate::ocr_vlm::SETUP_NOTICE;
    Ok(finish(text, failed && !setup_needed))
}

/// How a read is rendered: the MCP tool uses the defaults; the CLI prints
/// whole documents and can drop the front matter.
#[derive(Debug, Clone)]
pub struct ReadRender {
    pub front_matter: bool,
    /// No token budget unless max_tokens is given.
    pub unlimited: bool,
}

impl Default for ReadRender {
    fn default() -> Self {
        Self {
            front_matter: true,
            unlimited: false,
        }
    }
}

/// Read one source to Markdown text; returns (text, failed).
pub fn read_text(
    args: &ReadArgs,
    policy: &SourceAccessPolicy,
    render: &ReadRender,
) -> Result<(String, bool), String> {
    let budget = match (args.max_tokens, render.unlimited) {
        (None, true) => usize::MAX,
        (value, _) => budget_from(value),
    };
    let video = if let Some(selection) = &args.timeline {
        if args.download_asr_model.unwrap_or(false) {
            return Err("timeline never downloads models".into());
        }
        let ocr = args.ocr.filter(|s| s.enabled()).map(|s| s.engine());
        Some(crate::video_request::document(
            &args.source,
            selection,
            args.transcript.unwrap_or(false),
            ocr,
            policy,
        )?)
    } else {
        None
    };
    let manifest = video.as_ref().map(crate::video_request::manifest_id);
    let cursor_text = match (&manifest, args.cursor.as_deref()) {
        (Some(identity), Some(cursor)) => Some(
            cursor
                .strip_prefix(&format!("video:{identity}:"))
                .ok_or("video cursor source/manifest/options mismatch")?,
        ),
        (None, Some(cursor)) if cursor.starts_with("video:") => {
            return Err("video cursor requires matching timeline options".into())
        }
        (_, cursor) => cursor,
    };
    let cursor = cursor_text.map(Cursor::parse).transpose()?;
    let selection = match args
        .pages
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(spec) => selected_pages(&Some(PageSpecifier::Range(spec.to_string())))?,
        None => None,
    };
    let source = args.source.trim();
    if !is_url(source) {
        let admitted = policy.admit_path(source)?;
        if Path::new(&admitted).is_dir() {
            if args.node.is_some() {
                return Err("node requires a document, not a directory".into());
            }
            return Ok((list_directory(source, Path::new(&admitted)), false));
        }
    }
    let inherits_env = matches!(
        args.ocr,
        None | Some(crate::ocr_vlm::OcrSelection::Enabled(true))
    );
    let environment = if inherits_env {
        match std::env::var("ANYMD_OCR") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err("Invalid ANYMD_OCR".into()),
        }
    } else {
        None
    };
    let ocr_engine = crate::ocr_vlm::resolve_engine(args.ocr, environment.as_deref())?;
    let options = OpenOptions {
        ocr: args
            .ocr
            .map(|v| v.enabled())
            .or_else(|| args.node.as_ref().map(|_| false)),
        ocr_engine: Some(ocr_engine),
        transcript: args.transcript.unwrap_or(false) || args.download_asr_model.unwrap_or(false),
        download_asr_model: args.download_asr_model.unwrap_or(false),
        images: (args.wants_images() && (args.node.is_none() || args.images.is_some()))
            .then(anymd_formats::images::ImageStore::default_location)
            .flatten(),
        revisions: args.revisions(),
    };
    let opened = match video {
        Some(evidence) => Ok(Opened::from_video(source, &evidence)),
        None => Opened::open(source, policy, &options),
    };
    let read = match opened {
        Ok(mut opened) => read_opened(
            &mut opened,
            selection,
            cursor,
            budget,
            cursor.is_none(),
            args.node.as_deref(),
        ),
        Err(message) => failed(source, message),
    };
    if read
        .error
        .as_deref()
        .is_some_and(crate::ocr_vlm::is_setup_notice)
    {
        return Ok((format!("{}\n", crate::ocr_vlm::SETUP_NOTICE), true));
    }
    let mut out = if render.front_matter || read.error.is_some() {
        front_matter(&read.header)
    } else {
        String::new()
    };
    out.push_str(&read.body);
    if let Some(next) = read.next {
        let note = continuation_note(budget, next);
        out.push_str(&match manifest {
            Some(identity) => note.replace(
                &format!("\"{}\"", next.render()),
                &format!("\"video:{identity}:{}\"", next.render()),
            ),
            None => note,
        });
    }
    Ok((out.trim_end().to_string() + "\n", read.error.is_some()))
}

fn list_directory(label: &str, root: &Path) -> String {
    let mut files = Vec::new();
    let mut skipped = 0usize;
    for entry in WalkBuilder::new(root).max_depth(Some(6)).build().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if readable_extension(path) {
            if files.len() < 500 {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let shown = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string();
                files.push(format!("- {shown} ({})", human_size(size)));
            } else {
                skipped += 1;
            }
        }
    }
    let mut out = format!(
        "---\nsource: {}\ntype: directory\nreadable_files: {}\n---\n\n",
        yaml_value(label),
        files.len() + skipped
    );
    if files.is_empty() {
        out.push_str("No readable documents found (PDF, Office, EPUB, HTML, Markdown, CSV, images, media).\n");
    } else {
        out.push_str(&files.join("\n"));
        out.push('\n');
        if skipped > 0 {
            out.push_str(&format!("- … {skipped} more\n"));
        }
        out.push_str("\n<!-- Read one with read {source}, or search them all with search {query, sources: [this directory]}. -->\n");
    }
    out
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

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

pub(super) fn front_matter(header: &[(String, String)]) -> String {
    let mut out = String::from("---\n");
    for (key, value) in header {
        out.push_str(key);
        out.push_str(": ");
        if key == "scanned_pages" {
            out.push_str(value);
        } else {
            out.push_str(&yaml_value(value));
        }
        out.push('\n');
    }
    out.push_str("---\n\n");
    out
}
