//! Page selection, Markdown rendering and token-budgeted continuation of an opened document.

use crate::document::Opened;
use crate::lean::{estimate_tokens, scan_marker};

/// Below common MCP client output caps (Claude Code rejects results over 25k tokens).
pub const DEFAULT_MAX_TOKENS: usize = 20_000;
pub const MIN_MAX_TOKENS: usize = 500;
pub const MAX_MAX_TOKENS: usize = 1_000_000;

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

    pub(in crate::lean) fn render(self) -> String {
        if self.offset == 0 {
            self.page.to_string()
        } else {
            format!("{}:{}", self.page, self.offset)
        }
    }
}

pub(in crate::lean) fn budget_from(max_tokens: Option<u32>) -> usize {
    max_tokens
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_MAX_TOKENS)
        .clamp(MIN_MAX_TOKENS, MAX_MAX_TOKENS)
}

pub(in crate::lean) fn continuation_note(budget: usize, next: Cursor) -> String {
    if budget == usize::MAX {
        return format!("<!-- Continue with cursor: \"{}\" -->\n\n", next.render());
    }
    format!(
        "<!-- Stopped at the {budget}-token budget. Continue with cursor: \"{}\", or pick pages, or raise max_tokens. -->\n\n",
        next.render()
    )
}

pub(in crate::lean) struct SourceRead {
    pub(in crate::lean) header: Vec<(String, String)>,
    pub(in crate::lean) body: String,
    pub(in crate::lean) next: Option<Cursor>,
    pub(in crate::lean) shown: usize,
    pub(in crate::lean) error: Option<String>,
}

const SCAN_ALL_HINT: &str = "<!-- Every page shown is a scanned image with no text layer, so nothing was read; to enable local OCR, run `anymd setup ocr` (opts into downloading the OCR engine and ~2 GB of model weights), then retry with OCR enabled (`ocr: true` in MCP or `--ocr auto` in the CLI); documents stay on this machine -->";

const SCAN_ALL_OCR_HINT: &str =
    "<!-- Every page shown is a scanned image; OCR ran and found no text -->";

pub(in crate::lean) fn failed(label: &str, message: String) -> SourceRead {
    SourceRead {
        header: vec![
            ("source".into(), label.to_string()),
            ("error".into(), message.clone()),
        ],
        body: String::new(),
        next: None,
        shown: 0,
        error: Some(message),
    }
}

/// Read one opened document into Markdown within `budget` tokens.
pub(in crate::lean) fn read_opened(
    opened: &mut Opened,
    selection: Option<Vec<u32>>,
    cursor: Option<Cursor>,
    budget: usize,
    first_call: bool,
    node_id: Option<&str>,
) -> SourceRead {
    let navigation = match node_id {
        Some(id) => match crate::outline::Outline::build(opened) {
            Ok(outline) => match outline.nodes.iter().position(|n| n.id == id) {
                Some(index) => Some((outline, index)),
                None => {
                    return failed(
                        &opened.label,
                        format!("Unknown node {id}; call outline for this source."),
                    )
                }
            },
            Err(message) => return failed(&opened.label, message),
        },
        None => None,
    };
    let total = opened.total;
    let mut wanted: Vec<u32> = match selection {
        Some(pages) => pages
            .into_iter()
            .filter(|page| *page >= 1 && *page <= total)
            .collect(),
        None => (1..=total).collect(),
    };
    if let Some((outline, index)) = &navigation {
        let node = &outline.nodes[*index];
        wanted.retain(|page| *page >= node.from && *page <= node.to);
    }
    if let Some(cursor) = cursor {
        wanted.retain(|page| *page >= cursor.page);
    }
    let noun = opened.unit_noun;
    let mut header = vec![("source".to_string(), opened.label.clone())];
    if wanted.is_empty() {
        let message = format!(
            "No requested {} exist (the document has {total}).",
            plural(noun, 2)
        );
        header.push(("error".into(), message.clone()));
        return SourceRead {
            header,
            body: String::new(),
            next: None,
            shown: 0,
            error: Some(message),
        };
    }
    // One-unit documents (a web page, a DOCX, an image) need no unit markers.
    let markers = total > 1 || opened.is_paged();

    let mut body = String::new();
    let mut shown: Vec<u32> = Vec::new();
    let mut next = None;
    let mut used = 0usize;
    let mut visible = 0usize;
    let mut scanned: Vec<u32> = Vec::new();
    let chunk_size = if opened.is_paged() {
        PAGE_CHUNK
    } else {
        usize::MAX
    };
    'chunks: for chunk in wanted.chunks(chunk_size.min(wanted.len()).max(1)) {
        let units = match opened.units(chunk) {
            Ok(units) => units,
            Err(message) => return failed(&opened.label, message),
        };
        opened.title_from_units(&units);
        let chunk_scans = opened.scanned_pages(&units);
        for unit in units {
            let (clip_start, clip_end) = if let Some((outline, index)) = &navigation {
                let node = &outline.nodes[*index];
                let Some(range) = outline.units.iter().find(|r| r.number == unit.number) else {
                    continue;
                };
                let start = node.start.max(range.start);
                let end = node.end.min(range.end);
                if start >= end {
                    continue;
                }
                (
                    range.offset + start - range.start,
                    range.offset + end - range.start,
                )
            } else {
                (0, unit.markdown.len())
            };
            let mut skip = match cursor {
                Some(cursor) if cursor.page == unit.number => {
                    cursor.offset.min(unit.markdown.len())
                }
                _ => 0,
            };
            skip = skip.max(clip_start).min(clip_end);
            while !unit.markdown.is_char_boundary(skip) {
                skip -= 1;
            }
            let content = unit.markdown[skip..clip_end].trim_start();
            let visible_text = if navigation.is_some() {
                unit.markdown.as_str()
            } else {
                content
            };
            visible += visible_text.chars().filter(|c| c.is_alphanumeric()).count();
            let continued = if navigation.is_some() {
                cursor.is_some_and(|cursor| cursor.page == unit.number && cursor.offset > 0)
            } else {
                skip > 0
            };
            let mut marker = match (markers, continued) {
                (false, false) => String::new(),
                (false, true) => "<!-- continued -->\n\n".to_string(),
                (true, false) => format!("<!-- {} -->\n\n", unit.label),
                (true, true) => format!("<!-- {} (continued) -->\n\n", unit.label),
            };
            let is_scan = chunk_scans.contains(&unit.number) && !continued;
            if is_scan {
                marker.push_str(&scan_marker(unit.number, opened.ocr_ran(unit.number)));
            }
            // A scan page is charged its page label and scan marker, plus its
            // entry in the `scanned_pages` header; text pages keep the old cost.
            let piece_tokens = if is_scan {
                estimate_tokens(content) + estimate_tokens(&marker) + 4
            } else {
                estimate_tokens(content) + 8
            };
            if used + piece_tokens > budget {
                if shown.is_empty() {
                    // A unit larger than the whole budget: cut inside it.
                    let room_tokens = budget.saturating_sub(used + 8).max(250);
                    let room = content.len() * room_tokens / piece_tokens.max(1);
                    let cut = cut_point(content, room.max(512));
                    body.push_str(&marker);
                    body.push_str(content[..cut].trim_end());
                    body.push_str("\n\n");
                    shown.push(unit.number);
                    if is_scan {
                        scanned.push(unit.number);
                    }
                    let consumed = clip_end - content.len() + cut;
                    if cut < content.len() {
                        next = Some(Cursor {
                            page: unit.number,
                            offset: consumed,
                        });
                    } else if let Some(following) = wanted.iter().find(|p| **p > unit.number) {
                        next = Some(Cursor {
                            page: *following,
                            offset: 0,
                        });
                    }
                } else {
                    next = Some(Cursor {
                        page: unit.number,
                        offset: skip,
                    });
                }
                break 'chunks;
            }
            body.push_str(&marker);
            body.push_str(content.trim_end());
            body.push_str("\n\n");
            shown.push(unit.number);
            if is_scan {
                scanned.push(unit.number);
            }
            used += piece_tokens;
        }
    }

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
            header.push((
                "showing".into(),
                format!("{} {}", plural(noun, 2), describe_pages(&shown)),
            ));
        }
    }
    if !scanned.is_empty() {
        header.push((
            "scanned_pages".into(),
            format!(
                "[{}]",
                scanned
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
        if scanned.len() == shown.len() {
            let hint = if scanned.iter().all(|page| opened.ocr_ran(*page)) {
                SCAN_ALL_OCR_HINT
            } else {
                SCAN_ALL_HINT
            };
            body = format!("{hint}\n\n{body}");
        }
    }
    if opened.format == "pdf"
        && scanned.is_empty()
        && !shown.is_empty()
        && visible < shown.len() * 20
    {
        body.push_str(
            "<!-- These pages have little or no selectable text (scanned or image-only). \
Install `tesseract` for automatic OCR, or pass ocr: true. -->\n\n",
        );
    }
    if next.is_some() && first_call && navigation.is_none() && opened.format == "pdf" {
        let entries: Vec<String> = opened
            .outline()
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
        shown: shown.len(),
        error: None,
    }
}

pub(in crate::lean) fn describe_pages(pages: &[u32]) -> String {
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
pub(in crate::lean) fn cut_point(text: &str, limit: usize) -> usize {
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

fn plural(noun: &str, count: u32) -> String {
    if count == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}
