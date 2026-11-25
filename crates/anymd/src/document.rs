//! One entry point for every input anymd reads: a local path or an http(s)
//! URL becomes an [`Opened`] document whose citable units (pages, slides,
//! sheets, chapters) convert to Markdown on demand.

mod cache;
mod extensions;
mod images;
mod ocr;
mod open;
mod source;
#[cfg(test)]
mod tests;
mod types;
mod units;

pub use anymd_core::outline::Unit;
pub use extensions::{readable_extension, searchable_extension};
pub use images::{attach_structure_images, pdf_image_listing};
pub use source::is_url;
use source::{convert_other, read_head};
use types::Body;
pub use types::{EvidenceKind, OpenOptions, Opened};
