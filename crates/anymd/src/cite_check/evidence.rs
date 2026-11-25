use std::path::Path;
use std::time::Instant;

use anymd_core::text_index::cite_check::{
    check_cite_items_with_separator, CiteDecision, CiteNormalization,
};
use anymd_core::{text_index::PositionedTextItem, OcrPage};

use crate::schema::{Citation, InspectArgs};

use super::request::bounds;

pub(super) fn ocr_pages(
    args: &InspectArgs,
    snapshot: &Path,
    requested_pages: Vec<u32>,
    deadline: Instant,
    ocr_admitted: bool,
) -> (Vec<OcrPage>, bool) {
    let mut ocr_pages = Vec::new();
    let mut ocr_failed = args.ocr.is_some();
    if let Some(engine) = args.ocr.filter(|_| ocr_admitted) {
        let pdf_source = crate::schema::PdfSource {
            path: Some(snapshot.to_string_lossy().into()),
            url: None,
            pages: None,
        };
        if let Ok(source) = crate::visual_evidence::materialize_read_source(0, &pdf_source) {
            let outcomes = crate::ocr_evidence::run_read_ocr(
                &[(&source, requested_pages)],
                crate::ocr_evidence::ReadOcrOptions {
                    engine: Some(engine),
                    deadline: Some(deadline),
                    exact_geometry: true,
                    max_pages: 20,
                    timeout_ms: Some(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .as_millis() as u64,
                    ),
                    max_output_chars: args.max_output_chars.unwrap_or(200000) as usize,
                    ..Default::default()
                },
            );
            for outcome in outcomes {
                ocr_failed = outcome.error.is_some();
                ocr_pages.extend(outcome.pages);
            }
        }
    }
    (ocr_pages, ocr_failed)
}

pub(super) fn check_ocr(
    page: &OcrPage,
    citation: &Citation,
    normalization: CiteNormalization,
    ocr_failed: bool,
) -> CiteDecision {
    let items = ocr_items(page);
    let level = if page
        .words
        .as_ref()
        .is_some_and(|words| words.iter().any(|w| w.region_type.is_some()))
    {
        "ocr_region"
    } else {
        "ocr_word"
    };
    let complete = !ocr_failed
        && page.warnings.as_ref().is_none_or(Vec::is_empty)
        && items
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>()
            .join("")
            == page.text;
    let levels = page
        .words
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|word| !word.text.is_empty())
        .take(items.len())
        .map(|word| {
            if word.region_type.is_some() {
                "ocr_region"
            } else {
                "ocr_word"
            }
        })
        .collect::<Vec<_>>();
    check_cite_items_with_separator(
        &items,
        &citation.quote,
        bounds(citation),
        normalization,
        complete,
        level,
        "",
        Some(&levels),
    )
}

pub(super) fn ocr_items(page: &OcrPage) -> Vec<PositionedTextItem> {
    // Map the ordered provider words/regions to its actual original page text.
    // Only whitespace gaps are admissible. A disconnected region stops coverage;
    // an earlier, fully supported contiguous positive still stands.
    let mut items = Vec::new();
    let mut cursor = 0;
    for word in page.words.as_deref().unwrap_or_default() {
        if word.text.is_empty() {
            continue;
        }
        let Some(relative) = page.text[cursor..].find(&word.text) else {
            break;
        };
        let start = cursor + relative;
        if !page.text[cursor..start].chars().all(char::is_whitespace) {
            break;
        }
        let end = start + word.text.len();
        items.push(PositionedTextItem {
            text: page.text[cursor..end].into(),
            bounding_box: word
                .bounding_box
                .clone()
                .and_then(|v| serde_json::from_value(v).ok()),
            chars: Vec::new(),
            runs: Vec::new(),
        });
        cursor = end;
    }
    if page.text[cursor..].chars().all(char::is_whitespace) {
        if let Some(last) = items.last_mut() {
            last.text.push_str(&page.text[cursor..]);
        }
    }
    items
}
