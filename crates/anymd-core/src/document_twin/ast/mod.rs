//! Document AST: hierarchy, section context, aggregation, and visual enrichment attachment.

use serde_json::Value;

use self::aggregate::DocumentAstNode;
use super::PageText;

mod aggregate;
mod builder;
mod captions;
mod nodes;
mod page;

#[cfg(test)]
mod tests;

/// Build the v3.0.14 Document AST from the same semantic element and chunk
/// projections used by the public response. Optional provider-backed visual
/// enrichments attach to matching element targets or become page-local visual
/// nodes when the target element is not present on the page.
pub fn build_document_ast(
    pages: &[PageText],
    elements: &Value,
    chunks: &Value,
    warnings: &[String],
    visual_enrichments: &Value,
) -> Value {
    builder::build_document_ast(pages, elements, chunks, warnings, visual_enrichments)
}
