//! Provider-neutral OCR text-layer fusion for `read_pdf`.
//!
//! Provider configuration and execution remain in the MCP server. This module
//! owns only deterministic response semantics so every transport observes the
//! same source-ordinal result.

mod document_map;
mod fusion;
mod outcome;

pub use fusion::fuse_ocr_outcomes;
pub use outcome::{OcrPage, OcrWord, SourceOcrOutcome};

pub const OCR_STUB_WARNING: &str = "include_ocr_text_layer: provider execution is owned by anymd; pdf-reader-core omits ocr_text_layer until a normalized provider outcome is fused.";

#[cfg(test)]
mod tests;
