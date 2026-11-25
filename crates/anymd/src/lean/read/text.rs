//! Markdown reads from a document, URL, or directory.

use std::path::Path;

use rmcp::model::CallToolResult;

use crate::document::{is_url, EvidenceKind, OpenOptions, Opened};
use crate::page_selection::selected_pages;
use crate::schema::{PageSpecifier, ReadArgs};
use crate::source_access::SourceAccessPolicy;

use super::super::finish;
use super::directory::list_directory;
use super::front_matter;
use super::pagination::{budget_from, continuation_note, failed, read_opened, Cursor};

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
    read_text_with_evidence(args, policy, render).map(|(text, failed, _)| (text, failed))
}

pub(crate) fn read_text_with_evidence(
    args: &ReadArgs,
    policy: &SourceAccessPolicy,
    render: &ReadRender,
) -> Result<(String, bool, Option<EvidenceKind>), String> {
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
            return Ok((list_directory(source, Path::new(&admitted)), false, None));
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
    let (read, evidence) = match opened {
        Ok(mut opened) => {
            let read = read_opened(
                &mut opened,
                selection,
                cursor,
                budget,
                cursor.is_none(),
                args.node.as_deref(),
            );
            let evidence = opened.evidence_kind(read.shown);
            (read, evidence)
        }
        Err(message) => (failed(source, message), None),
    };
    if read
        .error
        .as_deref()
        .is_some_and(crate::ocr_vlm::is_setup_notice)
    {
        return Ok((format!("{}\n", crate::ocr_vlm::SETUP_NOTICE), true, None));
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
    Ok((
        out.trim_end().to_string() + "\n",
        read.error.is_some(),
        evidence,
    ))
}
