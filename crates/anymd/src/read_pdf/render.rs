use anymd_core::ReadPdfResponse;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};

pub(super) fn public_read_payload(payload: &ReadPdfResponse) -> Result<Value, rmcp::ErrorData> {
    let mut value = serde_json::to_value(payload).map_err(|error| {
        rmcp::ErrorData::internal_error(format!("Failed to serialize read_pdf: {error}"), None)
    })?;
    let Some(results) = value.get_mut("results").and_then(Value::as_array_mut) else {
        return Ok(value);
    };
    for result in results {
        let Some(data) = result.get_mut("data").and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(images) = data
            .remove("images")
            .and_then(|images| images.as_array().cloned())
        {
            let image_info = images
                .into_iter()
                .map(|image| {
                    json!({
                        "page": image.get("page").cloned().unwrap_or(Value::Null),
                        "index": image.get("index").cloned().unwrap_or(Value::Null),
                        "width": image.get("width").cloned().unwrap_or(Value::Null),
                        "height": image.get("height").cloned().unwrap_or(Value::Null),
                        "format": image.get("format").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect::<Vec<_>>();
            data.insert("image_info".into(), json!(image_info));
        }
        let Some(tables) = data
            .remove("tables")
            .and_then(|tables| tables.as_array().cloned())
        else {
            continue;
        };
        if tables.is_empty() {
            continue;
        }
        let table_info = tables
            .into_iter()
            .map(|table| {
                let row_count = table.get("rowCount").and_then(Value::as_u64).unwrap_or(0);
                let col_count = table.get("colCount").and_then(Value::as_u64).unwrap_or(0);
                let cell_count = table
                    .get("cells")
                    .and_then(Value::as_array)
                    .map_or(row_count.saturating_mul(col_count), |cells| {
                        cells.len() as u64
                    });
                let mut metadata = json!({
                    "page": table.get("page").cloned().unwrap_or(Value::Null),
                    "tableIndex": table.get("tableIndex").cloned().unwrap_or(Value::Null),
                    "rowCount": row_count,
                    "colCount": col_count,
                    "cellCount": cell_count,
                    "confidence": table.get("confidence").cloned().unwrap_or(Value::Null),
                });
                for field in ["bounding_box", "quality", "continuation", "provenance"] {
                    if let Some(field_value) = table.get(field) {
                        metadata[field] = field_value.clone();
                    }
                }
                metadata
            })
            .collect::<Vec<_>>();
        data.insert("table_info".into(), json!(table_info));
    }
    Ok(value)
}

pub(super) fn append_page_content(result: &mut CallToolResult, payload: &ReadPdfResponse) {
    for source in &payload.results {
        let Some(data) = source.data.as_ref() else {
            continue;
        };
        let images = data
            .images
            .as_ref()
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut emitted = vec![false; images.len()];
        let emit_text = data.markdown.is_none() && data.chunks.is_none();
        for page in data.page_texts.as_deref().unwrap_or(&[]) {
            if emit_text && !page.text.trim().is_empty() {
                result.content.push(ContentBlock::text(format!(
                    "[Page {}]\n{}",
                    page.page, page.text
                )));
            }
            for (index, image) in images.iter().enumerate().filter(|(_, image)| {
                image.get("page").and_then(Value::as_u64) == Some(u64::from(page.page))
            }) {
                let Some(encoded) = image.get("data").and_then(Value::as_str) else {
                    continue;
                };
                result
                    .content
                    .push(ContentBlock::image(encoded.to_string(), "image/png"));
                emitted[index] = true;
            }
        }
        for (index, image) in images.iter().enumerate() {
            if emitted[index] {
                continue;
            }
            let Some(encoded) = image.get("data").and_then(Value::as_str) else {
                continue;
            };
            result
                .content
                .push(ContentBlock::image(encoded.to_string(), "image/png"));
        }
    }
}

pub(super) fn append_table_content(result: &mut CallToolResult, payload: &ReadPdfResponse) {
    let tables = payload
        .results
        .iter()
        .filter_map(|source| source.data.as_ref())
        .filter_map(|data| data.tables.as_ref())
        .flat_map(|tables| tables.as_array().into_iter().flatten())
        .collect::<Vec<_>>();
    if tables.is_empty() {
        return;
    }
    let mut sections = vec!["## Extracted Tables".to_string(), String::new()];
    for table in tables {
        let page = table.get("page").and_then(Value::as_u64).unwrap_or(0);
        let table_number = table.get("tableIndex").and_then(Value::as_u64).unwrap_or(0) + 1;
        let confidence = table
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        sections.push(format!("### Page {page}, Table {table_number}"));
        sections.push(format!("*Confidence: {:.0}%*", confidence * 100.0));
        sections.push(String::new());
        if let Some(rows) = table.get("rows").and_then(Value::as_array) {
            for (row_index, row) in rows.iter().enumerate() {
                let cells = row
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|cell| {
                        let trimmed = cell.as_str().unwrap_or("").trim();
                        if trimmed.is_empty() {
                            " "
                        } else {
                            trimmed
                        }
                    })
                    .collect::<Vec<_>>();
                sections.push(format!("| {} |", cells.join(" | ")));
                if row_index == 0 {
                    sections.push(format!(
                        "| {} |",
                        cells.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")
                    ));
                }
            }
        }
        sections.push(String::new());
    }
    result.content.push(ContentBlock::text(sections.join("\n")));
}

pub(super) fn append_ocr_content(result: &mut CallToolResult, payload: &ReadPdfResponse) {
    for source in &payload.results {
        let Some(pages) = source
            .data
            .as_ref()
            .and_then(|data| data.ocr_text_layer.as_ref())
            .and_then(|layer| layer.get("pages"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for page in pages {
            let Some(text) = page.get("text").and_then(Value::as_str) else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            let page_number = page.get("page").and_then(Value::as_u64).unwrap_or(0);
            result.content.push(ContentBlock::text(format!(
                "[Page {page_number} OCR]\n{text}"
            )));
        }
    }
}
