//! anymd format converters: every non-PDF input becomes Markdown sections.
//!
//! Each module exposes `convert(bytes, &Options) -> Result<Converted, ConvertError>`.
//! A `Section` is the unit a reader paginates over and cites: a slide, a sheet,
//! an EPUB chapter, or a whole document when the format has no natural pages.

#[cfg(test)]
mod archive_validation;
pub mod asr;
pub mod cache;
mod critic;
pub mod csv;
mod detect;
mod dispatch;
pub mod docx;
mod encoding;
pub mod epub;
pub mod html;
pub mod image;
pub mod images;
pub mod limits;
mod markdown_table;
mod ooxml;
pub mod pptx;
mod revise;
#[cfg(feature = "native")]
mod tool;
#[cfg(not(feature = "native"))]
#[path = "tool_stub.rs"]
mod tool;
mod types;
pub mod video;
pub mod xlsx;

pub use detect::{detect, Format};
use dispatch::UnrestrictedSourceAccess;
pub use dispatch::{convert, convert_with_source_access};
pub(crate) use markdown_table::bounded_markdown_table;
pub use markdown_table::{markdown_table, table_cell};
pub use types::{
    ConversionLimit, ConvertError, Converted, Options, Revisions, Section, SourceAccess,
};
