//! Navigation over the canonical Markdown body and its citable units.

mod document;
mod headings;
mod sections;
mod tree;

#[cfg(test)]
mod tests;

pub use document::{
    front_matter, noun_for_units, title_from_units, unit_marker, DocumentInfo, Unit,
};
pub use headings::headings;
pub use tree::{Node, Outline, UnitRange};
