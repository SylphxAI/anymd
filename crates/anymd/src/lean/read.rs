//! Markdown read entry points for documents and directory listings.

mod directory;
pub(super) mod pagination;
mod pdf;
mod text;

pub use pdf::{read_pdf, read_wants_legacy};
pub use text::{read, read_text, ReadRender};

pub(crate) use pdf::scan_marker;
pub(crate) use text::read_text_with_evidence;

pub(super) fn front_matter(header: &[(String, String)]) -> String {
    anymd_core::outline::front_matter(header, &["scanned_pages"])
}
