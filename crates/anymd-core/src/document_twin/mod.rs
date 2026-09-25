//! Pure-Rust Agent Document Twin builders.
//!
//! These reconstruct the public `read_pdf` capability surface from selectable
//! text so pure-Rust MCP responses keep the same field names and shapes agents
//! already depend on. Geometry-heavy fields are best-effort without a layout
//! engine; provider-backed OCR/visual enrichments remain opt-in empty arrays
//! with explicit warnings (same fail-closed model as optional TS providers).
//!
//! These builders back the structured JSON path (`read_pdf` with `profile` or
//! `include_*` flags). The default Markdown path uses `anymd-pdf` instead.
//!
//! - `semantic`: semantic role hints for text lines
//! - `elements`: element projection, with tables and images in reading order
//! - `text_layer`: lines, words, and runs with UTF-16 offsets
//! - `chunks`: citation chunks
//! - `tables`: selectable-text table detection and continuation linking
//! - `diagnostics`: safety findings and layout diagnostics
//! - `trust`: trust report
//! - `ast`: Document AST, with caption linking and visual enrichment nodes
//! - `document_map`: per-page routing indexes and summary

mod ast;
mod chunks;
mod diagnostics;
mod document_map;
mod elements;
mod semantic;
mod tables;
mod text_layer;
mod trust;

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

use crate::text_index::PositionedTextItem;

pub use ast::build_document_ast;
pub use chunks::build_citation_chunks;
pub use diagnostics::{build_layout_diagnostics, build_safety_findings};
pub use document_map::build_document_map;
pub use elements::{
    build_elements_with_geometry, build_elements_with_tables_and_geometry,
    build_elements_with_tables_images_and_geometry,
};
pub(crate) use tables::build_tables_with_admission;
pub use text_layer::build_text_layer;
pub use trust::build_trust_report;

#[cfg(test)]
pub(crate) use elements::build_elements;

const TRUST_REPORT_VERSION: &str = "2026-06-15";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PageText {
    pub page: u32,
    pub text: String,
    #[serde(skip)]
    pub positioned_items: Vec<PositionedTextItem>,
}

fn pattern(slot: &'static OnceLock<Regex>, source: &str) -> &'static Regex {
    slot.get_or_init(|| Regex::new(source).expect("static semantic pattern is valid"))
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::PageText;

    pub(crate) fn pages(values: &[&str]) -> Vec<PageText> {
        values
            .iter()
            .enumerate()
            .map(|(index, text)| PageText {
                page: index as u32 + 1,
                text: (*text).to_string(),
                positioned_items: Vec::new(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_non_contiguous_original_page_identity() {
        let pages = vec![
            PageText {
                page: 2,
                text: "SECOND PAGE".into(),
                positioned_items: Vec::new(),
            },
            PageText {
                page: 7,
                text: "SEVENTH PAGE".into(),
                positioned_items: Vec::new(),
            },
        ];
        let elements = build_elements(&pages, true);
        assert_eq!(elements[0]["page"], 2);
        assert_eq!(elements[1]["page"], 7);
        let layout = build_layout_diagnostics(&pages);
        assert_eq!(layout[0]["page"], 2);
        assert_eq!(layout[1]["page"], 7);
        let trust = build_trust_report(
            &pages,
            &build_safety_findings(&pages),
            &layout,
            &json!([]),
            None,
            "standard",
        );
        assert_eq!(trust["summary"]["selected_pages"], json!([2, 7]));
    }
}
