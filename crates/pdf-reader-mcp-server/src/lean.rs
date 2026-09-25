//! Default agent-facing output: clean Markdown with page markers, a tiny
//! front-matter header, and a token budget with a continuation cursor.
//!
//! Evidence-heavy JSON (bounding boxes, hashes, document maps, audits) stays
//! available behind explicit options on the legacy routes.

<<<<<<< HEAD
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use rmcp::model::{CallToolResult, ContentBlock};

use crate::document::{is_url, readable_extension, searchable_extension, OpenOptions, Opened, Unit};
use crate::page_selection::selected_pages;
use crate::schema::{PageSpecifier, PdfSource, ReadArgs, ReadPdfArgs, SearchArgs, SearchPdfArgs};
use crate::source_access::SourceAccessPolicy;

const MAX_SEARCH_FILES: usize = 2000;
=======
use pdf_reader_core::markdown_layout::{load_document, pdf_to_markdown};
use rmcp::model::{CallToolResult, ContentBlock};

use crate::page_selection::selected_pages;
use crate::schema::{PdfSource, ReadPdfArgs, SearchPdfArgs};
use crate::visual_evidence::materialize_read_source;
>>>>>>> feat/lean-markdown-read

/// Below common MCP client output caps (Claude Code rejects results over 25k tokens).
pub const DEFAULT_MAX_TOKENS: usize = 20_000;
pub const MIN_MAX_TOKENS: usize = 500;
pub const MAX_MAX_TOKENS: usize = 1_000_000;

/// Cheap, slightly conservative estimate of LLM tokens (calibrated on o200k).
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_alphabetic() {
            let mut run: usize = 1;
            while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                chars.next();
                run += 1;
            }
            tokens += run.div_ceil(5);
        } else if ch.is_ascii_digit() {
            let mut run: usize = 1;
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
                run += 1;
            }
            tokens += run.div_ceil(3);
        } else if ch.is_whitespace() {
            let mut run: usize = 1;
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
                run += 1;
            }
            tokens += usize::from(run >= 3);
        } else {
            tokens += 1;
        }
    }
    tokens
}
const PAGE_CHUNK: usize = 12;
const OUTLINE_LIMIT: usize = 60;

/// Where a read resumes: 1-based page and a character offset inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub page: u32,
    pub offset: usize,
}

impl Cursor {
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim();
        let (page, offset) = match value.split_once(':') {
            Some((page, offset)) => (page, offset),
            None => (value, "0"),
        };
        let page = page
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|page| *page >= 1)
            .ok_or_else(|| {
                format!("Invalid cursor {value:?}: expected \"<page>\" or \"<page>:<offset>\".")
            })?;
        let offset = offset
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("Invalid cursor {value:?}: offset must be a number."))?;
        Ok(Self { page, offset })
    }

    fn render(self) -> String {
        if self.offset == 0 {
            self.page.to_string()
        } else {
            format!("{}:{}", self.page, self.offset)
        }
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

/// True when search needs the geometry/OCR JSON route.
pub fn search_wants_legacy(args: &SearchPdfArgs) -> bool {
    args.include_ocr_text_layer == Some(true) || args.detail == Some(true)
}

fn yaml_value(value: &str) -> String {
    let single_line = value.replace(['\r', '\n'], " ");
    let needs_quotes = single_line.contains(": ")
        || single_line.starts_with([
            '"', '\'', '[', '{', '#', '&', '*', '!', '|', '>', '%', '@', '`', '-', '?',
        ])
        || single_line.ends_with(':');
    if needs_quotes {
        serde_json::to_string(&single_line).unwrap_or(single_line)
    } else {
        single_line
    }
}

fn describe_pages(pages: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < pages.len() {
        let start = pages[index];
        let mut end = start;
        while index + 1 < pages.len() && pages[index + 1] == end + 1 {
            index += 1;
            end = pages[index];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        index += 1;
    }
    parts.join(",")
}

/// Cut `text` at a paragraph (or line, or char) boundary no later than `limit` bytes.
fn cut_point(text: &str, limit: usize) -> usize {
    if text.len() <= limit {
        return text.len();
    }
    let mut boundary = limit;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let head = &text[..boundary];
    if let Some(index) = head.rfind("\n\n").filter(|index| *index > limit / 2) {
        return index + 2;
    }
    if let Some(index) = head.rfind('\n').filter(|index| *index > limit / 2) {
        return index + 1;
    }
    boundary.max(1)
}

struct SourceRead {
    header: Vec<(String, String)>,
    body: String,
    next: Option<Cursor>,
    error: Option<String>,
}

<<<<<<< HEAD
fn front_matter(header: &[(String, String)]) -> String {
    let mut out = String::from("---\n");
    for (key, value) in header {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(&yaml_value(value));
        out.push('\n');
    }
    out.push_str("---\n\n");
    out
}

fn plural(noun: &str, count: u32) -> String {
    if count == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}

fn failed(label: &str, message: String) -> SourceRead {
    SourceRead {
        header: vec![("source".into(), label.to_string()), ("error".into(), message.clone())],
        body: String::new(),
        next: None,
        error: Some(message),
    }
}

/// Read one opened document into Markdown within `budget` tokens.
fn read_opened(
    opened: &mut Opened,
    selection: Option<Vec<u32>>,
=======
fn read_one_pdf(
    source: &PdfSource,
    source_index: usize,
>>>>>>> feat/lean-markdown-read
    cursor: Option<Cursor>,
    budget: usize,
    first_call: bool,
) -> SourceRead {
<<<<<<< HEAD
    let total = opened.total;
    let mut wanted: Vec<u32> = match selection {
        Some(pages) => pages.into_iter().filter(|page| *page >= 1 && *page <= total).collect(),
        None => (1..=total).collect(),
=======
    let label = source.label();
    let mut header = vec![("source".to_string(), label.clone())];
    let fail = |mut header: Vec<(String, String)>, message: String| {
        header.push(("error".into(), message.clone()));
        SourceRead {
            header,
            body: String::new(),
            next: None,
            error: Some(message),
        }
    };
    let owner = match materialize_read_source(source_index, source) {
        Ok(owner) => owner,
        Err(message) => return fail(header, message),
    };
    let doc = match load_document(owner.path()) {
        Ok(doc) => doc,
        Err(error) => return fail(header, error.message),
    };
    let total = pdf_reader_core::markdown_layout::page_count(&doc);
    let mut wanted = match selected_pages(&source.pages) {
        Ok(Some(pages)) => pages.into_iter().filter(|page| *page <= total).collect(),
        Ok(None) => (1..=total).collect::<Vec<u32>>(),
        Err(message) => return fail(header, message),
>>>>>>> feat/lean-markdown-read
    };
    if let Some(cursor) = cursor {
        wanted.retain(|page| *page >= cursor.page);
    }
<<<<<<< HEAD
    let noun = opened.unit_noun;
    let mut header = vec![("source".to_string(), opened.label.clone())];
    if wanted.is_empty() {
        let message = format!("No requested {} exist (the document has {total}).", plural(noun, 2));
        header.push(("error".into(), message.clone()));
        return SourceRead { header, body: String::new(), next: None, error: Some(message) };
    }
    // One-unit documents (a web page, a DOCX, an image) need no unit markers.
    let markers = total > 1 || opened.is_paged();
=======
    if wanted.is_empty() {
        header.push(("pages".into(), total.to_string()));
        return fail(
            header,
            format!("No requested pages exist (document has {total} pages)."),
        );
    }
>>>>>>> feat/lean-markdown-read

    let mut body = String::new();
    let mut shown: Vec<u32> = Vec::new();
    let mut next = None;
<<<<<<< HEAD
    let mut used = 0usize;
    let mut visible = 0usize;
    let chunk_size = if opened.is_paged() { PAGE_CHUNK } else { usize::MAX };
    'chunks: for chunk in wanted.chunks(chunk_size.min(wanted.len()).max(1)) {
        let units = match opened.units(chunk) {
            Ok(units) => units,
            Err(message) => return failed(&opened.label, message),
        };
        opened.title_from_units(&units);
        for unit in units {
            let mut skip = match cursor {
                Some(cursor) if cursor.page == unit.number => cursor.offset.min(unit.markdown.len()),
                _ => 0,
            };
            while !unit.markdown.is_char_boundary(skip) {
                skip -= 1;
            }
            let content = unit.markdown[skip..].trim_start();
            visible += content.chars().filter(|c| c.is_alphanumeric()).count();
            let marker = match (markers, skip > 0) {
                (false, false) => String::new(),
                (false, true) => "<!-- continued -->\n\n".to_string(),
                (true, false) => format!("<!-- {} -->\n\n", unit.label),
                (true, true) => format!("<!-- {} (continued) -->\n\n", unit.label),
=======
    let mut title = None;
    let mut text_chars = 0usize;
    let mut used = 0usize;
    'chunks: for chunk in wanted.chunks(PAGE_CHUNK) {
        let converted = match pdf_to_markdown(&doc, Some(chunk)) {
            Ok(converted) => converted,
            Err(error) => return fail(header, error.message),
        };
        if title.is_none() {
            title = converted.title.clone();
        }
        for page in converted.pages {
            let skip = match cursor {
                Some(cursor) if cursor.page == page.number => {
                    cursor.offset.min(page.markdown.len())
                }
                _ => 0,
            };
            let mut skip = skip;
            while !page.markdown.is_char_boundary(skip) {
                skip -= 1;
            }
            let content = page.markdown[skip..].trim_start();
            text_chars += content.chars().filter(|c| !c.is_whitespace()).count();
            let marker = if skip > 0 {
                format!("<!-- page {} (continued) -->\n\n", page.number)
            } else {
                format!("<!-- page {} -->\n\n", page.number)
>>>>>>> feat/lean-markdown-read
            };
            let piece_tokens = estimate_tokens(content) + 8;
            if used + piece_tokens > budget {
                if shown.is_empty() {
<<<<<<< HEAD
                    // A unit larger than the whole budget: cut inside it.
=======
                    // A single page larger than the whole budget: cut inside it.
>>>>>>> feat/lean-markdown-read
                    let room_tokens = budget.saturating_sub(used + 8).max(250);
                    let room = content.len() * room_tokens / piece_tokens.max(1);
                    let cut = cut_point(content, room.max(512));
                    body.push_str(&marker);
                    body.push_str(content[..cut].trim_end());
                    body.push_str("\n\n");
<<<<<<< HEAD
                    shown.push(unit.number);
                    let consumed = unit.markdown.len() - content.len() + cut;
                    if cut < content.len() {
                        next = Some(Cursor { page: unit.number, offset: consumed });
                    } else if let Some(following) = wanted.iter().find(|p| **p > unit.number) {
                        next = Some(Cursor { page: *following, offset: 0 });
                    }
                } else {
                    next = Some(Cursor { page: unit.number, offset: skip });
=======
                    shown.push(page.number);
                    let consumed = page.markdown.len() - content.len() + cut;
                    if cut < content.len() {
                        next = Some(Cursor {
                            page: page.number,
                            offset: consumed,
                        });
                    } else if let Some(following) = wanted.iter().find(|p| **p > page.number) {
                        next = Some(Cursor {
                            page: *following,
                            offset: 0,
                        });
                    }
                } else {
                    next = Some(Cursor {
                        page: page.number,
                        offset: skip,
                    });
>>>>>>> feat/lean-markdown-read
                }
                break 'chunks;
            }
            body.push_str(&marker);
            body.push_str(content.trim_end());
            body.push_str("\n\n");
<<<<<<< HEAD
            shown.push(unit.number);
=======
            shown.push(page.number);
>>>>>>> feat/lean-markdown-read
            used += piece_tokens;
        }
    }

<<<<<<< HEAD
    if opened.format != "pdf" {
        header.push(("format".into(), opened.format.to_string()));
    }
    if let Some(title) = &opened.title {
        header.push(("title".into(), title.clone()));
    }
    for (key, value) in opened.metadata.iter().take(6) {
        if key != "title" && key != "format" && !value.is_empty() && value.len() <= 200 {
            header.push((key.replace(' ', "_"), value.clone()));
        }
    }
    if markers {
        header.push((plural(noun, 2), total.to_string()));
        let complete = shown.len() == total as usize && next.is_none();
        if !complete {
            header.push(("showing".into(), format!("{} {}", plural(noun, 2), describe_pages(&shown))));
        }
    }
    if opened.format == "pdf" && !shown.is_empty() && visible < shown.len() * 20 {
        body.push_str(
            "<!-- These pages have little or no selectable text (scanned or image-only). \
Install `tesseract` for automatic OCR, or pass ocr: true. -->\n\n",
        );
    }
    if next.is_some() && first_call {
        let entries: Vec<String> = opened
            .outline()
=======
    if let Some(title) = title {
        header.push(("title".into(), title));
    }
    header.push(("pages".into(), total.to_string()));
    let all_pages = shown.len() == total as usize && next.is_none();
    if !all_pages {
        header.push((
            "showing".into(),
            format!("pages {}", describe_pages(&shown)),
        ));
    }
    if !shown.is_empty() && text_chars < shown.len() * 20 {
        body.push_str(
            "<!-- These pages have little or no selectable text (scanned or image-only). \
For OCR, call read_pdf with include_ocr_text_layer: true, or pdf_evidence with operation \"ocr_pages\". -->\n\n",
        );
    }
    if next.is_some() && first_call {
        let outline = pdf_reader_core::markdown_layout::outline(&doc);
        let entries: Vec<String> = outline
>>>>>>> feat/lean-markdown-read
            .iter()
            .filter(|(depth, _, _)| *depth <= 1)
            .take(OUTLINE_LIMIT)
            .map(|(depth, title, page)| {
                let indent = "  ".repeat(*depth);
                match page {
                    Some(page) => format!("{indent}- {title} (p. {page})"),
                    None => format!("{indent}- {title}"),
                }
            })
            .collect();
        if !entries.is_empty() {
            body = format!("<!-- outline -->\n{}\n\n{body}", entries.join("\n"));
        }
    }
<<<<<<< HEAD
    SourceRead { header, body, next, error: None }
}

fn budget_from(max_tokens: Option<u32>) -> usize {
    max_tokens
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_MAX_TOKENS)
        .clamp(MIN_MAX_TOKENS, MAX_MAX_TOKENS)
}

fn continuation_note(budget: usize, next: Cursor) -> String {
    format!(
        "<!-- Stopped at the {budget}-token budget. Continue with cursor: \"{}\", or pick pages, or raise max_tokens. -->\n\n",
        next.render()
    )
}

fn finish(out: String, all_failed: bool) -> CallToolResult {
    let text = out.trim_end().to_string() + "\n";
    if all_failed {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    }
}

/// `read`: any file, URL, or directory → Markdown.
pub fn read(args: &ReadArgs, policy: &SourceAccessPolicy) -> Result<CallToolResult, rmcp::ErrorData> {
    let budget = budget_from(args.max_tokens);
    let cursor = args
        .cursor
        .as_deref()
        .map(Cursor::parse)
        .transpose()
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    let selection = match args.pages.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(spec) => selected_pages(&Some(PageSpecifier::Range(spec.to_string())))
            .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?,
        None => None,
    };
    let source = args.source.trim();
    if !is_url(source) {
        let admitted = policy
            .admit_path(source)
            .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
        if Path::new(&admitted).is_dir() {
            return Ok(finish(list_directory(source, Path::new(&admitted)), false));
        }
    }
    let options = OpenOptions { ocr: args.ocr, transcript: args.transcript.unwrap_or(false) };
    let read = match Opened::open(source, policy, &options) {
        Ok(mut opened) => read_opened(&mut opened, selection, cursor, budget, cursor.is_none()),
        Err(message) => failed(source, message),
    };
    let mut out = front_matter(&read.header);
    out.push_str(&read.body);
    if let Some(next) = read.next {
        out.push_str(&continuation_note(budget, next));
    }
    Ok(finish(out, read.error.is_some()))
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
                let shown = path.strip_prefix(root).unwrap_or(path).display().to_string();
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

/// Legacy `read_pdf` default route: the same Markdown answer for PDF sources.
pub fn read_pdf(args: &ReadPdfArgs, policy: &SourceAccessPolicy) -> Result<CallToolResult, rmcp::ErrorData> {
    let budget = budget_from(args.max_tokens);
=======
    SourceRead {
        header,
        body,
        next,
        error: None,
    }
}

fn front_matter(header: &[(String, String)]) -> String {
    let mut out = String::from("---\n");
    for (key, value) in header {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(&yaml_value(value));
        out.push('\n');
    }
    out.push_str("---\n\n");
    out
}

pub fn read_pdf(args: &ReadPdfArgs) -> Result<CallToolResult, rmcp::ErrorData> {
    let budget = args
        .max_tokens
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_MAX_TOKENS)
        .clamp(MIN_MAX_TOKENS, MAX_MAX_TOKENS);
>>>>>>> feat/lean-markdown-read
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
<<<<<<< HEAD
=======

>>>>>>> feat/lean-markdown-read
    let mut out = String::new();
    let mut failures = 0usize;
    let mut skipped = Vec::new();
    for (index, source) in args.sources.iter().enumerate() {
        let remaining = budget.saturating_sub(estimate_tokens(&out));
        if index > 0 && remaining < 500 {
            skipped.push(source.label());
            continue;
        }
<<<<<<< HEAD
        let read = pdf_source_read(source, policy, cursor, remaining.max(500));
=======
        let read = read_one_pdf(
            source,
            index,
            cursor,
            remaining.max(500),
            cursor.is_none(),
        );
>>>>>>> feat/lean-markdown-read
        if read.error.is_some() {
            failures += 1;
        }
        out.push_str(&front_matter(&read.header));
        out.push_str(&read.body);
        if let Some(next) = read.next {
<<<<<<< HEAD
            out.push_str(&continuation_note(budget, next));
=======
            out.push_str(&format!(
                "<!-- Stopped at the {budget}-token budget. Continue with cursor: \"{}\" (same source), or pick pages with sources[].pages, or raise max_tokens. -->\n\n",
                next.render()
            ));
>>>>>>> feat/lean-markdown-read
        }
    }
    if !skipped.is_empty() {
        out.push_str(&format!(
            "<!-- Budget used up before these sources; read them separately: {} -->\n",
            skipped.join(", ")
        ));
    }
<<<<<<< HEAD
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
        Ok(mut opened) => read_opened(&mut opened, selection, cursor, budget, cursor.is_none()),
        Err(message) => failed(&label, message),
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Case-folded, whitespace-collapsed text with a map back to source byte offsets.
fn fold(text: &str, case_sensitive: bool) -> (String, Vec<usize>) {
=======
    let text = out.trim_end().to_string() + "\n";
    if failures == args.sources.len() {
        return Ok(CallToolResult::error(vec![ContentBlock::text(text)]));
    }
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn collapse_whitespace(text: &str) -> (String, Vec<usize>) {
    // Returns the collapsed text and, for each byte of it, the source byte offset.
>>>>>>> feat/lean-markdown-read
    let mut out = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len());
    let mut last_space = true;
    for (offset, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                map.push(offset);
                last_space = true;
            }
<<<<<<< HEAD
            continue;
        }
        last_space = false;
        let start = out.len();
        if case_sensitive {
            out.push(ch);
        } else {
            out.extend(ch.to_lowercase());
        }
        map.extend(std::iter::repeat_n(offset, out.len() - start));
    }
    map.push(text.len());
    (out, map)
}

fn is_word_char(ch: Option<char>) -> bool {
    ch.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// A snippet of `text` around [start, end) with every `highlights` range in bold.
fn snippet(text: &str, start: usize, end: usize, context: usize, highlights: &[(usize, usize)]) -> String {
    let mut from = start.saturating_sub(context);
    while !text.is_char_boundary(from) {
        from -= 1;
    }
    let mut to = (end + context).min(text.len());
    while !text.is_char_boundary(to) {
        to += 1;
    }
    // Snap to word boundaries so snippets do not start mid-word.
    if from > 0 {
        if let Some(space) = text[from..start].find(char::is_whitespace) {
            from += space + 1;
        }
    }
    if to < text.len() {
        if let Some(space) = text[end..to].rfind(char::is_whitespace) {
            to = end + space;
        }
    }
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    let mut cursor = from;
    let mut ranges: Vec<(usize, usize)> = highlights
        .iter()
        .copied()
        .filter(|(s, e)| *s >= from && *e <= to && s < e)
        .collect();
    ranges.sort_unstable();
    for (s, e) in ranges {
        if s < cursor {
            continue;
        }
        out.push_str(&text[cursor..s]);
        out.push_str("**");
        out.push_str(&text[s..e]);
        out.push_str("**");
        cursor = e;
    }
    out.push_str(&text[cursor..to]);
    if to < text.len() {
        out.push('…');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct SearchDoc {
    label: String,
    noun: &'static str,
    total: u32,
    units: std::sync::Arc<Vec<Unit>>,
}

fn locator(doc: &SearchDoc, unit: &Unit) -> String {
    if doc.total <= 1 && doc.noun != "page" {
        String::new()
    } else if doc.noun == "page" {
        format!(" p.{}", unit.number)
    } else {
        format!(" {}", unit.label)
    }
}

fn expand_sources(
    specs: &[String],
    policy: &SourceAccessPolicy,
    glob: Option<&str>,
    notes: &mut Vec<String>,
) -> Result<Vec<(String, String)>, String> {
    // (display label, spec to open)
    let mut files = Vec::new();
    for spec in specs {
        let spec = spec.trim();
        if is_url(spec) {
            files.push((spec.to_string(), spec.to_string()));
            continue;
        }
        let admitted = policy.admit_path(spec)?;
        let root = PathBuf::from(&admitted);
        if !root.is_dir() {
            files.push((spec.to_string(), admitted));
            continue;
        }
        let mut builder = WalkBuilder::new(&root);
        builder.follow_links(false);
        if let Some(glob) = glob {
            let mut overrides = OverrideBuilder::new(&root);
            overrides.add(glob).map_err(|error| format!("invalid glob {glob:?}: {error}"))?;
            builder.overrides(overrides.build().map_err(|error| error.to_string())?);
        }
        let mut count = 0usize;
        for entry in builder.build().flatten() {
            let path = entry.path();
            if !path.is_file() || !searchable_extension(path) {
                continue;
            }
            if files.len() >= MAX_SEARCH_FILES {
                count += 1;
                continue;
            }
            let shown = Path::new(spec)
                .join(path.strip_prefix(&root).unwrap_or(path))
                .display()
                .to_string();
            files.push((shown, path.display().to_string()));
        }
        if count > 0 {
            notes.push(format!("{count} files under {spec} were not searched (limit {MAX_SEARCH_FILES})."));
        }
    }
    Ok(files)
}

fn load_search_docs(
    files: Vec<(String, String)>,
    policy: &SourceAccessPolicy,
    notes: &mut Vec<String>,
) -> Vec<SearchDoc> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8)
        .min(files.len().max(1));
    let options = OpenOptions { ocr: Some(false), transcript: false };
    let mut results: Vec<Option<Result<SearchDoc, String>>> = (0..files.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let files = &files;
                let options = &options;
                scope.spawn(move || {
                    files
                        .iter()
                        .enumerate()
                        .skip(worker)
                        .step_by(workers)
                        .map(|(index, (label, spec))| {
                            let doc = Opened::open(spec, policy, options).and_then(|opened| {
                                Ok(SearchDoc {
                                    label: label.clone(),
                                    noun: opened.unit_noun,
                                    total: opened.total,
                                    units: opened.all_units()?,
                                })
                            });
                            (index, doc)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            if let Ok(done) = handle.join() {
                for (index, doc) in done {
                    results[index] = Some(doc);
                }
            }
        }
    });
    let mut docs = Vec::new();
    for (result, (label, _)) in results.into_iter().zip(&files) {
        match result {
            Some(Ok(doc)) => docs.push(doc),
            Some(Err(message)) => notes.push(format!("{label}: {message}")),
            None => notes.push(format!("{label}: failed to open")),
        }
    }
    docs
}

fn terms_of(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() && !is_cjk(ch) {
            word.extend(ch.to_lowercase());
        } else {
            if !word.is_empty() {
                terms.push(std::mem::take(&mut word));
            }
            if is_cjk(ch) {
                terms.push(ch.to_string());
            }
        }
    }
    if !word.is_empty() {
        terms.push(word);
    }
    terms
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

struct Hit<'a> {
    doc: &'a SearchDoc,
    unit: &'a Unit,
    snippet: String,
}

fn literal_hits<'a>(
    docs: &'a [SearchDoc],
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    context: usize,
    max_per_doc: usize,
) -> (Vec<(usize, usize, Vec<Hit<'a>>)>, usize) {
    // Per document: (doc index, total count, shown hits).
    let (needle, _) = fold(query, case_sensitive);
    let mut out = Vec::new();
    let mut grand_total = 0;
    for (doc_index, doc) in docs.iter().enumerate() {
        let mut count = 0;
        let mut hits = Vec::new();
        for unit in doc.units.iter() {
            let (hay, map) = fold(&unit.markdown, case_sensitive);
            let mut from = 0;
            while let Some(found) = hay[from..].find(&needle) {
                let start = from + found;
                let end = start + needle.len();
                from = end;
                if whole_word
                    && (is_word_char(hay[..start].chars().next_back()) || is_word_char(hay[end..].chars().next()))
                {
                    continue;
                }
                count += 1;
                if hits.len() < max_per_doc {
                    let (s, e) = (map[start], map[end.min(map.len() - 1)]);
                    let e = if e <= s { s + 1 } else { e };
                    let mut e = e.min(unit.markdown.len());
                    while !unit.markdown.is_char_boundary(e) {
                        e += 1;
                    }
                    hits.push(Hit { doc, unit, snippet: snippet(&unit.markdown, s, e, context, &[(s, e)]) });
                }
            }
        }
        grand_total += count;
        if count > 0 {
            out.push((doc_index, count, hits));
        }
    }
    (out, grand_total)
}

fn ranked_hits<'a>(docs: &'a [SearchDoc], query: &str, context: usize, limit: usize) -> Vec<(f64, Hit<'a>)> {
    let mut query_terms = terms_of(query);
    query_terms.sort();
    query_terms.dedup();
    if query_terms.is_empty() {
        return Vec::new();
    }
    let mut units: Vec<(&SearchDoc, &Unit, HashMap<String, usize>, usize)> = Vec::new();
    for doc in docs {
        for unit in doc.units.iter() {
            let terms = terms_of(&unit.markdown);
            let len = terms.len();
            let mut tf: HashMap<String, usize> = HashMap::new();
            for term in terms {
                if query_terms.binary_search(&term).is_ok() {
                    *tf.entry(term).or_default() += 1;
                }
            }
            units.push((doc, unit, tf, len));
        }
    }
    let n = units.len() as f64;
    let avg = units.iter().map(|u| u.3 as f64).sum::<f64>() / n.max(1.0);
    let idf: HashMap<&String, f64> = query_terms
        .iter()
        .map(|term| {
            let df = units.iter().filter(|u| u.2.contains_key(term)).count() as f64;
            (term, ((n - df + 0.5) / (df + 0.5) + 1.0).ln())
        })
        .collect();
    let (k1, b) = (1.2, 0.75);
    let mut scored: Vec<(f64, usize)> = units
        .iter()
        .enumerate()
        .filter_map(|(index, (_, _, tf, len))| {
            let score: f64 = tf
                .iter()
                .map(|(term, f)| {
                    let f = *f as f64;
                    idf[term] * f * (k1 + 1.0) / (f + k1 * (1.0 - b + b * *len as f64 / avg.max(1.0)))
                })
                .sum();
            (score > 0.0).then_some((score, index))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(limit);
    scored
        .into_iter()
        .map(|(score, index)| {
            let (doc, unit, _, _) = &units[index];
            // Highlight query terms; center on the rarest one present.
            let lower = unit.markdown.to_lowercase();
            let same_len = lower.len() == unit.markdown.len();
            let mut highlights = Vec::new();
            let mut anchor: Option<(f64, usize, usize)> = None;
            if same_len {
                for term in &query_terms {
                    let mut from = 0;
                    while let Some(found) = lower[from..].find(term.as_str()) {
                        let start = from + found;
                        let end = start + term.len();
                        from = end;
                        let bounded = is_cjk(term.chars().next().unwrap_or(' '))
                            || (!is_word_char(lower[..start].chars().next_back())
                                && !is_word_char(lower[end..].chars().next()));
                        if bounded {
                            highlights.push((start, end));
                            let weight = idf[term];
                            if anchor.is_none_or(|(w, _, _)| weight > w) {
                                anchor = Some((weight, start, end));
                            }
=======
        } else {
            let start = out.len();
            out.push(ch);
            map.extend(std::iter::repeat_n(offset, out.len() - start));
            last_space = false;
        }
    }
    (out, map)
}

fn is_word_byte(text: &str, index: usize, before: bool) -> bool {
    let ch = if before {
        text[..index].chars().next_back()
    } else {
        text[index..].chars().next()
    };
    ch.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

fn snippet(page_text: &str, start: usize, end: usize, context: usize) -> String {
    let mut from = start.saturating_sub(context);
    while !page_text.is_char_boundary(from) {
        from -= 1;
    }
    let mut to = (end + context).min(page_text.len());
    while !page_text.is_char_boundary(to) {
        to += 1;
    }
    let clean = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "{}{}**{}**{}{}",
        if from > 0 { "…" } else { "" },
        clean(&page_text[from..start]).trim_start().to_string()
            + if page_text[..start].ends_with(char::is_whitespace) {
                " "
            } else {
                ""
            },
        clean(&page_text[start..end]),
        if page_text[end..].starts_with(char::is_whitespace) {
            " "
        } else {
            ""
        }
        .to_string()
            + clean(&page_text[end..to]).trim_end(),
        if to < page_text.len() { "…" } else { "" },
    )
}

pub fn search_pdf(args: &SearchPdfArgs) -> Result<CallToolResult, rmcp::ErrorData> {
    let case_sensitive = args.case_sensitive.unwrap_or(false);
    let whole_word = args.whole_word.unwrap_or(false);
    let max_matches = args.max_matches_per_source.unwrap_or(20) as usize;
    let context = args.context_chars.unwrap_or(80) as usize;
    let max_pages = args.max_pages.unwrap_or(1000) as usize;
    let (query, _) = collapse_whitespace(args.query.trim());
    let needle = if case_sensitive {
        query.clone()
    } else {
        query.to_lowercase()
    };
    if needle.is_empty() {
        return Err(rmcp::ErrorData::invalid_params(
            "query must not be empty.",
            None,
        ));
    }

    let mut out = String::new();
    let mut failures = 0;
    for (index, source) in args.sources.iter().enumerate() {
        let label = source.label();
        let result = (|| -> Result<(Vec<(u32, String)>, usize, u32, usize), String> {
            let owner = materialize_read_source(index, source)?;
            let doc = load_document(owner.path()).map_err(|error| error.message)?;
            let total = pdf_reader_core::markdown_layout::page_count(&doc);
            let mut pages: Vec<u32> = match selected_pages(&source.pages)? {
                Some(pages) => pages.into_iter().filter(|page| *page <= total).collect(),
                None => (1..=total).collect(),
            };
            pages.truncate(max_pages);
            let mut hits = Vec::new();
            let mut count = 0usize;
            for chunk in pages.chunks(PAGE_CHUNK) {
                let converted =
                    pdf_to_markdown(&doc, Some(chunk)).map_err(|error| error.message)?;
                for page in converted.pages {
                    let (text, _) = collapse_whitespace(&page.markdown);
                    let haystack = if case_sensitive {
                        text.clone()
                    } else {
                        text.to_lowercase()
                    };
                    if haystack.len() != text.len() {
                        // Case folding changed byte lengths; fall back to exact positions only.
                    }
                    let mut from = 0;
                    while let Some(found) = haystack[from..].find(&needle) {
                        let start = from + found;
                        let end = start + needle.len();
                        from = end.max(start + 1);
                        while from < haystack.len() && !haystack.is_char_boundary(from) {
                            from += 1;
                        }
                        if whole_word
                            && (is_word_byte(&haystack, start, true)
                                || is_word_byte(&haystack, end, false))
                        {
                            continue;
                        }
                        count += 1;
                        if hits.len() < max_matches && haystack.len() == text.len() {
                            hits.push((page.number, snippet(&text, start, end, context)));
                        } else if hits.len() < max_matches {
                            hits.push((page.number, snippet(&haystack, start, end, context)));
                        }
                        if from >= haystack.len() {
                            break;
>>>>>>> feat/lean-markdown-read
                        }
                    }
                }
            }
<<<<<<< HEAD
            let (start, end) = anchor.map(|(_, s, e)| (s, e)).unwrap_or((0, 0));
            (score, Hit { doc, unit, snippet: snippet(&unit.markdown, start, end, context, &highlights) })
        })
        .collect()
}

fn run_search(
    specs: &[String],
    query: &str,
    mode: &str,
    options: SearchOptions,
    policy: &SourceAccessPolicy,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let query = query.trim();
    if query.is_empty() {
        return Err(rmcp::ErrorData::invalid_params("query must not be empty.", None));
    }
    let mut notes = Vec::new();
    let files = expand_sources(specs, policy, options.glob.as_deref(), &mut notes)
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    if files.is_empty() {
        let mut out = format!("No searchable documents found in {}.\n", specs.join(", "));
        for note in notes {
            out.push_str(&format!("- {note}\n"));
        }
        return Ok(finish(out, true));
    }
    let docs = load_search_docs(files, policy, &mut notes);
    let units_searched: usize = docs.iter().map(|d| d.units.len()).sum();
    let scope = format!(
        "{} file{}, {} section{} searched",
        docs.len(),
        if docs.len() == 1 { "" } else { "s" },
        units_searched,
        if units_searched == 1 { "" } else { "s" }
    );
    let single = docs.len() == 1;
    let mut out = String::new();
    let mut literal_total = 0;
    if mode != "ranked" {
        let (per_doc, total) = literal_hits(
            &docs,
            query,
            options.case_sensitive,
            options.whole_word,
            options.context,
            options.max_results,
        );
        literal_total = total;
        if total > 0 || mode == "literal" {
            out.push_str(&format!(
                "{total} match{} for \"{query}\" ({scope})\n",
                if total == 1 { "" } else { "es" }
            ));
            let mut shown = 0;
            for (_, count, hits) in &per_doc {
                if shown >= options.max_results {
                    break;
                }
                let doc = hits[0].doc;
                if !single {
                    out.push_str(&format!("\n### {} ({count})\n", doc.label));
                }
                for hit in hits.iter().take(options.max_results - shown) {
                    let loc = locator(hit.doc, hit.unit);
                    let loc = loc.trim_start();
                    if loc.is_empty() {
                        out.push_str(&format!("- {}\n", hit.snippet));
                    } else {
                        out.push_str(&format!("- {loc}: {}\n", hit.snippet));
                    }
                    shown += 1;
                }
                if *count > hits.len() {
                    out.push_str(&format!("- … {} more in this file\n", count - hits.len()));
                }
            }
        }
    }
    if mode == "ranked" || (mode == "auto" && literal_total == 0) {
        let ranked = ranked_hits(&docs, query, options.context, options.max_results);
        if mode == "auto" {
            out.push_str(&format!("No exact match for \"{query}\" ({scope}). "));
        }
        if ranked.is_empty() {
            out.push_str("No passages contain any of the query words.\n");
        } else {
            out.push_str(&format!("Best matching passages (BM25, top {}):\n", ranked.len()));
            for (rank, (score, hit)) in ranked.iter().enumerate() {
                let file = if single { String::new() } else { format!("{} ", hit.doc.label) };
                let loc = locator(hit.doc, hit.unit);
                out.push_str(&format!(
                    "{}. {}{} (score {:.1}): {}\n",
                    rank + 1,
                    file,
                    loc.trim_start(),
                    score,
                    hit.snippet
                ));
            }
        }
    }
    if !notes.is_empty() {
        out.push_str("\n<!-- skipped: ");
        out.push_str(&notes.join("; "));
        out.push_str(" -->\n");
    }
    Ok(finish(out, docs.is_empty()))
}

struct SearchOptions {
    case_sensitive: bool,
    whole_word: bool,
    context: usize,
    max_results: usize,
    glob: Option<String>,
}

/// `search`: literal or ranked search across files, directories, and URLs.
pub fn search(args: &SearchArgs, policy: &SourceAccessPolicy) -> Result<CallToolResult, rmcp::ErrorData> {
    let mode = args.mode.as_deref().unwrap_or("auto").to_ascii_lowercase();
    if !matches!(mode.as_str(), "auto" | "literal" | "ranked") {
        return Err(rmcp::ErrorData::invalid_params("mode must be auto, literal, or ranked.", None));
    }
    let sources = if args.sources.is_empty() { vec![".".to_string()] } else { args.sources.clone() };
    run_search(
        &sources,
        &args.query,
        &mode,
        SearchOptions {
            case_sensitive: args.case_sensitive.unwrap_or(false),
            whole_word: args.whole_word.unwrap_or(false),
            context: args.context_chars.unwrap_or(80) as usize,
            max_results: args.max_results.unwrap_or(20).clamp(1, 500) as usize,
            glob: args.glob.clone(),
        },
        policy,
    )
}

/// Legacy `search_pdf` default route: literal search with page locators.
pub fn search_pdf(args: &SearchPdfArgs, policy: &SourceAccessPolicy) -> Result<CallToolResult, rmcp::ErrorData> {
    let specs: Vec<String> = args.sources.iter().map(PdfSource::label).collect();
    run_search(
        &specs,
        &args.query,
        "literal",
        SearchOptions {
            case_sensitive: args.case_sensitive.unwrap_or(false),
            whole_word: args.whole_word.unwrap_or(false),
            context: args.context_chars.unwrap_or(80) as usize,
            max_results: args.max_matches_per_source.unwrap_or(20) as usize,
            glob: None,
        },
        policy,
    )
=======
            Ok((hits, count, total, pages.len()))
        })();
        match result {
            Ok((hits, count, total, searched)) => {
                let scope = if searched < total as usize {
                    format!(" (searched {searched} of {total} pages)")
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "## {label}\n{count} match{} for \"{}\"{scope}\n",
                    if count == 1 { "" } else { "es" },
                    args.query
                ));
                for (page, text) in &hits {
                    out.push_str(&format!("- p.{page}: {text}\n"));
                }
                if count > hits.len() {
                    out.push_str(&format!(
                        "- … {} more (raise max_matches_per_source or narrow pages)\n",
                        count - hits.len()
                    ));
                }
                out.push('\n');
            }
            Err(message) => {
                failures += 1;
                out.push_str(&format!("## {label}\nerror: {message}\n\n"));
            }
        }
    }
    let text = out.trim_end().to_string() + "\n";
    if failures == args.sources.len() {
        return Ok(CallToolResult::error(vec![ContentBlock::text(text)]));
    }
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
>>>>>>> feat/lean-markdown-read
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips() {
        assert_eq!(Cursor::parse("7").unwrap(), Cursor { page: 7, offset: 0 });
        assert_eq!(
            Cursor::parse("3:1200").unwrap(),
            Cursor {
                page: 3,
                offset: 1200
            }
        );
        assert_eq!(Cursor::parse("3:1200").unwrap().render(), "3:1200");
        assert!(Cursor::parse("0").is_err());
        assert!(Cursor::parse("x").is_err());
    }

    #[test]
    fn estimates_tokens_conservatively() {
        assert_eq!(estimate_tokens("hello world"), 2);
        assert_eq!(estimate_tokens("12345"), 2);
        assert_eq!(estimate_tokens("| a |"), 3);
        assert_eq!(estimate_tokens("注意力"), 3);
    }

    #[test]
    fn describes_page_runs() {
        assert_eq!(describe_pages(&[1, 2, 3, 5, 7, 8]), "1-3,5,7-8");
    }

    #[test]
    fn cuts_at_paragraph_boundaries() {
        let text = "aaaa\n\nbbbb\n\ncccc";
        assert_eq!(&text[..cut_point(text, 12)], "aaaa\n\nbbbb\n\n");
    }

    #[test]
    fn snippet_bolds_the_match() {
        let text = "The Transformer uses multi-head attention everywhere.";
        let start = text.find("multi-head").unwrap();
<<<<<<< HEAD
        let end = start + "multi-head attention".len();
        let s = snippet(text, start, end, 10, &[(start, end)]);
=======
        let s = snippet(text, start, start + "multi-head attention".len(), 10);
>>>>>>> feat/lean-markdown-read
        assert!(s.contains("**multi-head attention**"), "{s}");
    }

    #[test]
<<<<<<< HEAD
    fn folding_maps_back_to_source_offsets() {
        let (folded, map) = fold("Multi-Head\n  Attention", false);
        assert_eq!(folded, "multi-head attention");
        assert_eq!(map[11], 13);
    }

    #[test]
    fn terms_split_words_and_cjk() {
        assert_eq!(terms_of("Self-Attention 注意"), ["self", "attention", "注", "意"]);
    }

    fn text_of(result: &CallToolResult) -> String {
        result
            .content
            .iter()
            .filter_map(|block| block.as_text().map(|text| text.text.clone()))
            .collect()
    }

    fn fixture_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("notes.md"),
            "# Notes\n\nThe transformer uses multi-head attention.\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("prices.csv"), "item,price\napple,1.20\npear,0.95\n").unwrap();
        std::fs::write(
            dir.path().join("page.html"),
            "<html><head><title>Guide</title></head><body><main><h1>Guide</h1><p>Attention heads attend to tokens.</p></main></body></html>",
        )
        .unwrap();
        dir
    }

    #[test]
    fn read_converts_a_csv_to_a_table() {
        let dir = fixture_dir();
        let args = ReadArgs {
            source: dir.path().join("prices.csv").display().to_string(),
            pages: None,
            max_tokens: None,
            cursor: None,
            ocr: None,
            transcript: None,
        };
        let text = text_of(&read(&args, &SourceAccessPolicy::unrestricted()).unwrap());
        assert!(text.contains("format: csv"), "{text}");
        assert!(text.contains("| apple | 1.20 |"), "{text}");
    }

    #[test]
    fn read_lists_a_directory() {
        let dir = fixture_dir();
        let args = ReadArgs {
            source: dir.path().display().to_string(),
            pages: None,
            max_tokens: None,
            cursor: None,
            ocr: None,
            transcript: None,
        };
        let text = text_of(&read(&args, &SourceAccessPolicy::unrestricted()).unwrap());
        assert!(text.contains("readable_files: 3"), "{text}");
        assert!(text.contains("- notes.md"), "{text}");
    }

    #[test]
    fn search_finds_literal_hits_across_formats_then_ranks() {
        let dir = fixture_dir();
        let mut args = SearchArgs {
            query: "multi-head attention".into(),
            sources: vec![dir.path().display().to_string()],
            mode: None,
            glob: None,
            case_sensitive: None,
            whole_word: None,
            max_results: None,
            context_chars: None,
        };
        let policy = SourceAccessPolicy::unrestricted();
        let text = text_of(&search(&args, &policy).unwrap());
        assert!(text.starts_with("1 match for \"multi-head attention\" (3 files"), "{text}");
        assert!(text.contains("**multi-head attention**"), "{text}");

        args.query = "attention tokens heads".into();
        let text = text_of(&search(&args, &policy).unwrap());
        assert!(text.contains("No exact match"), "{text}");
        assert!(text.contains("page.html"), "{text}");

        args.query = "pear".into();
        args.glob = Some("*.csv".into());
        let text = text_of(&search(&args, &policy).unwrap());
        assert!(text.contains("1 file"), "{text}");
    }

    #[test]
=======
>>>>>>> feat/lean-markdown-read
    fn yaml_values_quote_when_needed() {
        assert_eq!(
            yaml_value("Attention Is All You Need"),
            "Attention Is All You Need"
        );
        assert_eq!(yaml_value("BERT: Pre-training"), "\"BERT: Pre-training\"");
    }
}
