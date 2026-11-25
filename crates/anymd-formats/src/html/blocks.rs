//! Block rendering entry point and shared, bounded Markdown accumulation.

use crate::ConvertError;

use super::inline::finish_inline;
use super::Ctx;

mod list;
mod pre;
mod render;
mod table;

pub(super) use render::render_block;
use render::render_children;

#[derive(Default)]
pub(super) struct Blocks {
    blocks: Vec<String>,
    inline: String,
    bytes: usize,
    failure: Option<ConvertError>,
}

impl Blocks {
    fn flush(&mut self) {
        let paragraph = finish_inline(&self.inline);
        self.inline.clear();
        if !paragraph.is_empty() {
            self.push_checked(paragraph);
        }
    }

    fn push(&mut self, block: String) {
        self.flush();
        if !block.trim().is_empty() {
            self.push_checked(block);
        }
    }

    fn push_checked(&mut self, block: String) {
        if self.failure.is_some() {
            return;
        }
        self.bytes = self.bytes.saturating_add(block.len());
        let separators = self.blocks.len().saturating_mul(2);
        if self.bytes.saturating_add(separators) > crate::limits::MAX_OUTPUT_BYTES {
            self.failure = Some(ConvertError::limit(
                "output",
                crate::limits::MAX_OUTPUT_BYTES,
            ));
        } else {
            self.blocks.push(block);
        }
    }

    pub(super) fn finish(mut self, separator: &str) -> Result<String, ConvertError> {
        self.flush();
        if let Some(error) = self.failure {
            return Err(error);
        }
        Ok(self.blocks.join(separator))
    }
}

fn finish_blocks(blocks: Blocks, separator: &str, ctx: &Ctx<'_>) -> String {
    blocks.finish(separator).unwrap_or_else(|error| {
        *ctx.failure.borrow_mut() = Some(error);
        String::new()
    })
}
