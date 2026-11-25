//! Rust-native search_pdf extraction for anymd.

mod execute;
mod ocr_fusion;
mod request;
mod result;
mod source;
#[cfg(test)]
mod tests;
mod types;

pub use execute::{search_pdf, search_pdf_from_value};
pub use ocr_fusion::fuse_search_ocr_outcomes;
pub use types::{
    SearchPdfError, SearchPdfErrorCode, SearchPdfInput, SearchPdfResponse, SearchPdfSource,
    SEARCH_PDF_ROUTE,
};

const DEFAULT_MAX_PAGES: u32 = 100;
const DEFAULT_MAX_MATCHES: u32 = 50;
const DEFAULT_CONTEXT_CHARS: u32 = 120;
