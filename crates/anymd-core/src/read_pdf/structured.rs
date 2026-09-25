//! Structured Markdown and HTML rendering of read_pdf output, and its rebuild after OCR fusion.

use super::*;

pub(super) fn render_structured_markdown(
    pages: &[crate::document_twin::PageText],
    tables: &Value,
    images: &Value,
) -> String {
    let mut parts = pages
        .iter()
        .map(|page| {
            let items = if page.positioned_items.is_empty() {
                page.text
                    .lines()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            } else {
                page.positioned_items
                    .iter()
                    .map(|item| item.text.trim())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            };
            let mut lines = vec![format!("## Page {}", page.page), String::new()];
            for item in items {
                lines.push(item.to_string());
                lines.push(String::new());
            }
            for image in images.as_array().into_iter().flatten().filter(|image| {
                image.get("page").and_then(Value::as_u64) == Some(u64::from(page.page))
            }) {
                let index = image.get("index").and_then(Value::as_u64).unwrap_or(0) + 1;
                let width = image.get("width").and_then(Value::as_u64).unwrap_or(0);
                let height = image.get("height").and_then(Value::as_u64).unwrap_or(0);
                let format = image.get("format").and_then(Value::as_str).unwrap_or("");
                lines.push(format!("[Image {index}: {width}x{height} {format}]"));
                lines.push(String::new());
            }
            lines.join("\n").trim_end().to_string()
        })
        .collect::<Vec<_>>();
    let table_values = tables.as_array().into_iter().flatten().collect::<Vec<_>>();
    if !table_values.is_empty() {
        let mut table_parts = vec!["## Extracted Tables".to_string(), String::new()];
        for table in table_values {
            let page = table.get("page").and_then(Value::as_u64).unwrap_or(0);
            let index = table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0) + 1;
            let confidence = table
                .get("confidence")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            table_parts.push(format!("### Page {page}, Table {index}"));
            table_parts.push(format!("*Confidence: {:.0}%*", confidence * 100.0));
            table_parts.push(String::new());
            let mut lines = Vec::new();
            for (row_index, row) in table
                .get("rows")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let cells = row
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|cell| {
                        let trimmed = cell.as_str().unwrap_or("").trim();
                        if trimmed.is_empty() {
                            " ".to_string()
                        } else {
                            trimmed.to_string()
                        }
                    })
                    .collect::<Vec<_>>();
                lines.push(format!("| {} |", cells.join(" | ")));
                if row_index == 0 {
                    lines.push(format!(
                        "| {} |",
                        cells.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")
                    ));
                }
            }
            if !lines.is_empty() {
                table_parts.push(lines.join("\n"));
                table_parts.push(String::new());
            }
        }
        parts.push(table_parts.join("\n"));
    }
    parts.join("\n\n").trim().to_string()
}

pub(super) fn render_structured_html(
    pages: &[crate::document_twin::PageText],
    tables: &Value,
    images: &Value,
) -> String {
    let mut parts = pages
        .iter()
        .map(|page| {
            let items = if page.positioned_items.is_empty() {
                page.text
                    .lines()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            } else {
                page.positioned_items
                    .iter()
                    .map(|item| item.text.trim())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
            };
            let mut body = vec![
                format!("<section data-page=\"{}\">", page.page),
                format!("<h2>Page {}</h2>", page.page),
            ];
            body.extend(
                items
                    .into_iter()
                    .map(|item| format!("<p>{}</p>", html_escape(item))),
            );
            for image in images.as_array().into_iter().flatten().filter(|image| {
                image.get("page").and_then(Value::as_u64) == Some(u64::from(page.page))
            }) {
                let index = image.get("index").and_then(Value::as_u64).unwrap_or(0);
                let width = image.get("width").and_then(Value::as_u64).unwrap_or(0);
                let height = image.get("height").and_then(Value::as_u64).unwrap_or(0);
                let format = html_escape(image.get("format").and_then(Value::as_str).unwrap_or(""));
                body.push(format!(
                    "<figure data-image-index=\"{index}\">\n<figcaption>Image {}: {width}x{height} {format}</figcaption>\n</figure>",
                    index + 1
                ));
            }
            body.push("</section>".into());
            body.join("\n")
        })
        .collect::<Vec<_>>();
    for table in tables.as_array().into_iter().flatten() {
        let rows = table
            .get("rows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|row| {
                let cells = row
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|cell| format!("<td>{}</td>", html_escape(cell.as_str().unwrap_or(""))))
                    .collect::<String>();
                format!("<tr>{cells}</tr>")
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!(
            "<table data-page=\"{}\" data-table-index=\"{}\">\n<tbody>\n{rows}\n</tbody>\n</table>",
            table.get("page").and_then(Value::as_u64).unwrap_or(0),
            table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0)
        ));
    }
    parts.join("\n\n").trim().to_string()
}

pub(crate) fn rebuild_structured_outputs(
    data: &mut ReadPdfData,
    context: &StructuredFusionContext,
    tables: &Value,
) {
    use crate::document_twin::{
        build_citation_chunks, build_document_ast, build_document_map,
        build_elements_with_tables_and_geometry,
    };

    let output_elements = build_elements_with_tables_and_geometry(
        &context.pages,
        tables,
        context.semantic_hints,
        context.page_geometry.as_ref(),
    );
    let semantic_elements = if context.semantic_hints {
        output_elements.clone()
    } else {
        build_elements_with_tables_and_geometry(
            &context.pages,
            tables,
            true,
            context.page_geometry.as_ref(),
        )
    };
    let output_chunks = build_citation_chunks(&output_elements, context.semantic_hints);
    let internal_chunks = if context.emit_chunks {
        output_chunks.clone()
    } else {
        build_citation_chunks(&semantic_elements, true)
    };
    let ast_warnings = data.warnings.clone().unwrap_or_default();
    let text_layer = crate::document_twin::build_text_layer(&context.pages);
    let images = data.images.clone().unwrap_or_else(|| json!([]));
    if context.emit_markdown {
        data.markdown = Some(render_structured_markdown(&context.pages, tables, &images));
    }
    if context.emit_html {
        data.html = Some(render_structured_html(&context.pages, tables, &images));
    }
    if context.emit_chunks {
        data.chunks = Some(output_chunks.clone());
    }
    if context.emit_elements {
        data.elements = Some(output_elements.clone());
    }
    if context.emit_tables {
        data.tables = Some(tables.clone());
    }
    if context.emit_document_ast {
        let visual_enrichments = data.visual_enrichments.clone().unwrap_or_else(|| json!([]));
        data.document_ast = Some(build_document_ast(
            &context.pages,
            &semantic_elements,
            &internal_chunks,
            &ast_warnings,
            &visual_enrichments,
        ));
    }
    if context.emit_document_map {
        data.document_map = Some(build_document_map(
            &context.pages,
            context.total_pages,
            &semantic_elements,
            &internal_chunks,
            &context.safety,
            &context.layout,
            &text_layer,
            context.page_geometry.as_ref(),
            &ast_warnings,
            context.trust.as_ref(),
            context.accessibility.as_ref(),
            &context.visual_candidates,
        ));
    }
}

pub(super) fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
