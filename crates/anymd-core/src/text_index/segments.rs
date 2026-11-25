//! Collect bounded glyph geometry and normalize it into positioned text segments.

mod assemble;
mod collect;
mod normalize;
mod raw_part;

pub(super) use collect::TextItemOutput;
pub(super) use normalize::normalize_page_text_parts;
pub(super) use raw_part::{GlyphExtent, RawTextPart};
