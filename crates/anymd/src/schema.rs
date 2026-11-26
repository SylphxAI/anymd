//! Typed MCP tool argument schemas (schemars -> tools/list inputSchema).
//!
//! This module intentionally mirrors the immutable TypeScript v3.0.14 schemas.
//! `schemars` produces the client-visible contract; the `validate` methods close
//! the gap between descriptive JSON Schema and serde's structural decoding.

mod common;
mod evidence;
mod inspect;
mod lean;
mod legacy;

pub use common::{
    PageNumber, PageSpecifier, PdfSource, ReadPdfAutoDetail, TrustReportRedaction,
    MAX_SOURCES_PER_REQUEST,
};
pub use evidence::{
    PdfEvidenceArgs, PdfEvidenceOperation, PdfEvidenceRegion, PdfEvidenceSource, RegionBoundingBox,
};
pub use inspect::{Citation, CiteBoundingBox, CiteNormalization, InspectArgs, InspectOperation};
pub use lean::{OutlineArgs, ReadArgs, SearchArgs};
pub use legacy::{ComparePdfArgs, ReadPdfArgs, SearchPdfArgs};

#[cfg(test)]
mod tests;
