//! Selectable tables, elements, chunks, and text-layer data for the selected pages.

use serde_json::{json, Value};

use crate::document_twin::{
    build_citation_chunks, build_elements_with_tables_images_and_geometry,
    build_tables_with_admission, build_text_layer, PageText,
};

use super::options::ExtractionOptions;

pub(super) struct PageContent {
    pub(super) tables: Option<Value>,
    pub(super) elements: Option<Value>,
    pub(super) semantic_elements: Option<Value>,
    pub(super) chunks: Option<Value>,
    pub(super) internal_chunks: Option<Value>,
    pub(super) text_layer: Option<Value>,
}

pub(super) fn build_page_content(
    pages: &[PageText],
    options: &ExtractionOptions,
    page_geometry: Option<&Value>,
    images: Option<&Value>,
    warnings: &mut Vec<String>,
) -> PageContent {
    let page_content_table_geometry = options.full_text
        || options.elements
        || options.semantic_hints
        || options.markdown
        || options.html
        || options.chunks
        || options.text_layer
        || options.ocr
        || options.images
        || options.safety
        || options.layout
        || options.document_map
        || options.document_ast
        || options.visual
        || options.trust
        || options.accessibility;
    let tables = if options.tables
        || options.document_ast
        || options.document_map
        || options.visual
        || options.trust
    {
        let (tables, table_warnings) =
            build_tables_with_admission(pages, page_content_table_geometry);
        warnings.extend(table_warnings);
        Some(tables)
    } else {
        None
    };
    let empty_array = json!([]);
    let table_values = tables.as_ref().unwrap_or(&empty_array);
    let image_values = images.unwrap_or(&empty_array);
    let plain_elements =
        ((options.elements || options.chunks) && !options.semantic_hints).then(|| {
            build_elements_with_tables_images_and_geometry(
                pages,
                table_values,
                image_values,
                false,
                page_geometry,
            )
        });
    let semantic_elements = (options.semantic_hints
        || options.document_ast
        || options.document_map
        || options.visual
        || options.trust
        || options.accessibility)
        .then(|| {
            build_elements_with_tables_images_and_geometry(
                pages,
                table_values,
                image_values,
                true,
                page_geometry,
            )
        });
    let elements = if options.elements || options.semantic_hints {
        if options.semantic_hints {
            semantic_elements.clone()
        } else {
            plain_elements.clone()
        }
    } else {
        None
    };
    let chunks = if options.chunks {
        let chunk_elements = if options.semantic_hints {
            semantic_elements.as_ref()
        } else {
            plain_elements.as_ref()
        };
        Some(build_citation_chunks(
            chunk_elements.unwrap_or(&empty_array),
            options.semantic_hints,
        ))
    } else {
        None
    };
    let internal_chunks = if options.document_ast || options.document_map {
        chunks.clone().or_else(|| {
            Some(build_citation_chunks(
                semantic_elements.as_ref().unwrap_or(&empty_array),
                true,
            ))
        })
    } else {
        None
    };
    let text_layer = (options.text_layer || options.document_map).then(|| build_text_layer(pages));
    PageContent {
        tables,
        elements,
        semantic_elements,
        chunks,
        internal_chunks,
        text_layer,
    }
}
