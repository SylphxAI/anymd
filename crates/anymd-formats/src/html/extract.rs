//! HTML/XHTML parser preparation, page metadata, and content selection.

mod elements;
mod head;
mod main_content;
mod pruning;
mod xhtml;

pub(super) use elements::{element_name, is_block, BLOCK_TAGS};
pub(super) use head::{head_facts, resolve_base};
pub(super) use main_content::select_main;
pub(super) use pruning::is_dropped;
pub(super) use xhtml::expand_self_closing;

#[derive(Default)]
pub(super) struct HeadFacts {
    pub(super) title: Option<String>,
    pub(super) description: Option<String>,
    pub(super) author: Option<String>,
    pub(super) published: Option<String>,
    pub(super) base_href: Option<String>,
}
