//! HTML → Markdown tuned for agents: main content only, no chrome, few tokens.
//!
//! A purpose-built walker over `scraper`/html5ever rather than a generic
//! turndown port: it needs main-content selection, noise pruning by role and
//! class, relative-URL resolution, and pipe tables built from block-level
//! cells, which a generic rule set does not give.

use std::cell::RefCell;

use scraper::Html;
use url::Url;

use crate::images::Embed;
use crate::{ConvertError, Converted, Options, Section};

mod blocks;
mod extract;
mod inline;
#[cfg(test)]
mod tests;

use blocks::{render_block, Blocks};
use extract::{element_name, expand_self_closing, head_facts, resolve_base, select_main};
use inline::{text_len, tidy};

/// Deeper trees than this are flattened to text so recursion stays bounded.
const MAX_DEPTH: usize = 256;

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    let text = crate::encoding::text(bytes, "HTML")?;
    let doc = Html::parse_document(&expand_self_closing(&text));
    let head = head_facts(&doc);
    let base = resolve_base(options.base_url.as_deref(), head.base_href.as_deref());
    let markdown = render_document(
        &doc,
        &RenderOptions {
            base,
            keep_relative_links: true,
            select_main: true,
            images: None,
        },
    )?;
    let mut metadata = Vec::new();
    if let Some(description) = head.description {
        metadata.push(("description".to_string(), description));
    }
    if let Some(author) = head.author {
        metadata.push(("author".to_string(), author));
    }
    if let Some(published) = head.published {
        metadata.push(("published".to_string(), published));
    }
    Ok(Converted {
        outline: Vec::new(),
        format: "html".into(),
        title: head.title,
        sections: vec![Section {
            label: "document".into(),
            markdown,
        }],
        metadata,
    })
}

type ImageRenderer<'a> = dyn Fn(&str, &str) -> Embed + 'a;

/// Rendering knobs shared with the EPUB converter.
pub(crate) struct RenderOptions<'a> {
    pub base: Option<Url>,
    /// Keep links whose target is relative and cannot be resolved (EPUB drops them:
    /// they point into the archive and only cost tokens).
    pub keep_relative_links: bool,
    /// Narrow to `<main>`/`<article>` when the page has one.
    pub select_main: bool,
    /// Decides what an `<img>` becomes, given its `src` and alt text: an
    /// exported file, nothing, or the default alt-text form (EPUB).
    pub images: Option<&'a ImageRenderer<'a>>,
}

/// Markdown for an already-decoded HTML/XHTML document.
pub(crate) fn render_html(text: &str, options: &RenderOptions<'_>) -> Result<String, ConvertError> {
    render_document(&Html::parse_document(&expand_self_closing(text)), options)
}

fn render_document(doc: &Html, options: &RenderOptions<'_>) -> Result<String, ConvertError> {
    let root = doc.tree.root();
    let body = root
        .descendants()
        .find(|n| element_name(*n) == Some("body"))
        .unwrap_or(root);
    let content = if options.select_main {
        select_main(body)
    } else {
        body
    };
    let ctx = Ctx {
        failure: RefCell::new(None),
        options,
        root_text: text_len(content).max(1),
        root: content.id(),
    };
    let mut out = Blocks::default();
    render_block(content, &ctx, &mut out, 0);
    if let Some(error) = ctx.failure.into_inner() {
        return Err(error);
    }
    let markdown = tidy(&out.finish("\n\n")?);
    crate::limits::check_output(&markdown)?;
    Ok(markdown)
}

struct Ctx<'a> {
    failure: RefCell<Option<ConvertError>>,
    options: &'a RenderOptions<'a>,
    root_text: usize,
    root: ego_tree::NodeId,
}
