//! Assemble read metadata, scan notices, and the optional PDF bookmark preview.

use crate::document::Opened;

use super::{describe_pages, Cursor, SourceRead};

const OUTLINE_LIMIT: usize = 60;

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

/// Add response headers and document-level notes after the units are read.
pub(super) fn finish(
    opened: &Opened,
    mut body: String,
    shown: &[u32],
    scanned: &[u32],
    visible: usize,
    next: Option<Cursor>,
    show_outline: bool,
) -> SourceRead {
    let noun = opened.unit_noun;
    let total = opened.total;
    let info = opened.info();
    let mut header = info.header(&opened.metadata);
    if info.has_markers() {
        header.push((plural(noun, 2), total.to_string()));
        let complete = shown.len() == total as usize && next.is_none();
        if !complete {
            header.push((
                "showing".into(),
                format!("{} {}", plural(noun, 2), describe_pages(shown)),
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
    if next.is_some() && show_outline && opened.format == "pdf" {
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

pub(super) fn plural(noun: &str, count: u32) -> String {
    if count == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}
