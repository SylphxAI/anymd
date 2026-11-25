//! Select the primary content node before rendering.

use ego_tree::NodeRef;
use scraper::Node;

use super::super::inline::text_len;

/// Prefer `<main>`, `role=main`, a dominant `<article>`, or a well-known content
/// container; fall back to `<body>` with noise pruning.
pub(in crate::html) fn select_main(body: NodeRef<'_, Node>) -> NodeRef<'_, Node> {
    let body_len = text_len(body).max(1);
    let substantial = |node: NodeRef<'_, Node>| {
        let len = text_len(node);
        len >= 200 && len * 4 >= body_len
    };

    let mut mains = Vec::new();
    let mut articles = Vec::new();
    let mut known = Vec::new();
    for node in body.descendants() {
        let Some(el) = node.value().as_element() else {
            continue;
        };
        if el.name() == "main" || el.attr("role") == Some("main") {
            mains.push(node);
        } else if el.name() == "article" {
            articles.push(node);
        } else if let Some(id) = el.id() {
            if matches!(
                id,
                "content"
                    | "main-content"
                    | "maincontent"
                    | "mw-content-text"
                    | "article"
                    | "article-body"
            ) {
                known.push(node);
            }
        }
    }

    // A single dominant article beats a <main> that also wraps listings/related links.
    let article_total: usize = articles.iter().map(|a| text_len(*a)).sum();
    if let Some(best) = articles.iter().copied().max_by_key(|a| text_len(*a)) {
        if substantial(best) && text_len(best) * 10 >= article_total * 6 {
            return best;
        }
    }
    if let Some(main) = mains.into_iter().find(|m| substantial(*m)) {
        return main;
    }
    if let Some(node) = known.into_iter().find(|n| substantial(*n)) {
        return node;
    }
    body
}
