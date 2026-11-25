//! Document AST caption-to-target linking under a request-wide comparison budget.

mod geometry;
mod link;

pub(super) use link::ast_link_captions_on_page;

pub(super) const MAX_AST_CAPTION_LINK_COMPARISONS: usize = 65_536;
pub(super) const AST_CAPTION_LINK_BUDGET_WARNING: &str =
    "Document AST caption linking stopped at the Rust request-wide comparison limit.";

#[derive(Debug)]
pub(super) struct AstCaptionLinkBudget {
    remaining: usize,
    pub(super) exhausted: bool,
}

impl AstCaptionLinkBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            exhausted: false,
        }
    }

    fn admit_comparison(&mut self) -> bool {
        let Some(remaining) = self.remaining.checked_sub(1) else {
            self.exhausted = true;
            return false;
        };
        self.remaining = remaining;
        true
    }
}
