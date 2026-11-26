pub mod cite_check;
pub mod cli;
mod command_provider;
pub mod discover_compat;
pub mod document;
pub mod evidence;
pub mod http_transport;
pub mod lean;
mod mcp;
mod ocr_evidence;
pub mod ocr_vlm;
pub mod outline;
mod page_selection;
pub mod pdf_compare;
pub mod pdf_evidence;
mod pdf_evidence_common;
pub mod pro;
mod provider_budget;
pub mod read_pdf;
mod region_analysis_evidence;
pub mod schema;
mod schema_sanitize;
pub mod search;
pub mod setup;
pub mod source_access;
pub mod video_evidence;
pub mod video_request;
mod visual_evidence;

pub use mcp::{PdfReaderMcp, LEGACY_TOOL_NAMES};

pub const SERVER_NAME: &str = "anymd";
/// The product version: the workspace version, set with `bun scripts/set-version.ts`.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// GitHub release assets, `<base>/v<version>/<asset>`: the VLM OCR companion
/// and its checksum manifest.
pub const RELEASE_DOWNLOAD_BASE: &str = "https://github.com/SylphxAI/anymd/releases/download";
pub const SERVER_INFO_META_KEY: &str = "io.modelcontextprotocol/serverInfo";
pub const SERVER_INSTRUCTIONS: &str =
    "Local document reader for agents. read turns any file, URL, or directory listing into clean \
Markdown (PDF, Office, EPUB, HTML, CSV, images, media) with page/slide/sheet markers and a cursor \
for long documents. outline returns a document tree; read node fetches a section. search finds text across files and directories with page locators and node title paths. inspect \
renders, crops, OCRs, diffs, or returns structured JSON for PDFs. No cloud API key is required. \
anymd Pro ($29 once, `anymd pro buy` or https://anymd.sylphx.com/pricing) adds inspect cite_check (accepts read/search output directly) and video_timeline / render_frame for verifiable video evidence.";

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
mod sidecar_access_tests;
