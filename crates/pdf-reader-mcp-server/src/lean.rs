//! Default agent-facing output: clean Markdown with page markers, a tiny
//! front-matter header, and a token budget with a continuation cursor.
//!
//! Evidence-heavy JSON (bounding boxes, hashes, document maps, audits) stays
//! available behind explicit options on the legacy routes.

use pdf_reader_core::markdown_layout::{load_document, pdf_to_markdown};
use rmcp::model::{CallToolResult, ContentBlock};

use crate::page_selection::selected_pages;
use crate::schema::{PdfSource, ReadPdfArgs, SearchPdfArgs};
use crate::visual_evidence::materialize_read_source;

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

fn read_one_pdf(
    source: &PdfSource,
    source_index: usize,
    cursor: Option<Cursor>,
    budget: usize,
    first_call: bool,
) -> SourceRead {
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
    };
    if let Some(cursor) = cursor {
        wanted.retain(|page| *page >= cursor.page);
    }
    if wanted.is_empty() {
        header.push(("pages".into(), total.to_string()));
        return fail(
            header,
            format!("No requested pages exist (document has {total} pages)."),
        );
    }

    let mut body = String::new();
    let mut shown: Vec<u32> = Vec::new();
    let mut next = None;
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
            };
            let piece_tokens = estimate_tokens(content) + 8;
            if used + piece_tokens > budget {
                if shown.is_empty() {
                    // A single page larger than the whole budget: cut inside it.
                    let room_tokens = budget.saturating_sub(used + 8).max(250);
                    let room = content.len() * room_tokens / piece_tokens.max(1);
                    let cut = cut_point(content, room.max(512));
                    body.push_str(&marker);
                    body.push_str(content[..cut].trim_end());
                    body.push_str("\n\n");
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
                }
                break 'chunks;
            }
            body.push_str(&marker);
            body.push_str(content.trim_end());
            body.push_str("\n\n");
            shown.push(page.number);
            used += piece_tokens;
        }
    }

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
        let read = read_one_pdf(
            source,
            index,
            cursor,
            remaining.max(500),
            cursor.is_none(),
        );
        if read.error.is_some() {
            failures += 1;
        }
        out.push_str(&front_matter(&read.header));
        out.push_str(&read.body);
        if let Some(next) = read.next {
            out.push_str(&format!(
                "<!-- Stopped at the {budget}-token budget. Continue with cursor: \"{}\" (same source), or pick pages with sources[].pages, or raise max_tokens. -->\n\n",
                next.render()
            ));
        }
    }
    if !skipped.is_empty() {
        out.push_str(&format!(
            "<!-- Budget used up before these sources; read them separately: {} -->\n",
            skipped.join(", ")
        ));
    }
    let text = out.trim_end().to_string() + "\n";
    if failures == args.sources.len() {
        return Ok(CallToolResult::error(vec![ContentBlock::text(text)]));
    }
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn collapse_whitespace(text: &str) -> (String, Vec<usize>) {
    // Returns the collapsed text and, for each byte of it, the source byte offset.
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
                        }
                    }
                }
            }
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
        let s = snippet(text, start, start + "multi-head attention".len(), 10);
        assert!(s.contains("**multi-head attention**"), "{s}");
    }

    #[test]
    fn yaml_values_quote_when_needed() {
        assert_eq!(
            yaml_value("Attention Is All You Need"),
            "Attention Is All You Need"
        );
        assert_eq!(yaml_value("BERT: Pre-training"), "\"BERT: Pre-training\"");
    }
}
