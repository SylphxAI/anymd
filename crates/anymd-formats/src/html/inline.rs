//! Inline HTML rendering and the text helpers shared with block rendering.

mod links;
mod markdown;
mod render;
mod text;

pub(super) use markdown::{finish_inline, longest_run, push_text, tidy};
pub(super) use render::{inline_string, render_inline};
pub(super) use text::{collapse, plain_text, text_len};
