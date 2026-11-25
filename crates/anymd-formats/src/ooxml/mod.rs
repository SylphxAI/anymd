//! Shared plumbing for the Office Open XML converters: a bounded zip reader,
//! a tiny XML tree, relationship resolution, core properties, and the inline
//! run renderer that turns formatted text into compact Markdown.

mod package;
mod render;
mod xml;

pub(crate) use package::{
    guard_zip, invalid, validate_archive, EntryInflation, Package, Rels, MAX_ENTRIES,
};
pub(crate) use render::{collapse_ws, Blocks, Inline, ListIndent, Media};
pub(crate) use xml::{core_properties, parse_xml, Element, Node};

#[allow(unused_imports, reason = "retain the original package imports")]
pub(crate) use package::{check_limits, Rel, MAX_ENTRY_BYTES};
#[allow(unused_imports, reason = "retain the original render imports")]
pub(crate) use render::{image_markdown, Span};
#[allow(unused_imports, reason = "retain the original XML import")]
pub(crate) use xml::local;

#[cfg(test)]
mod tests;
