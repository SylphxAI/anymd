use anymd_core::{
    fuse_ocr_outcomes, fuse_visual_outcomes, hash_file, read_pdf_from_value, ReadPdfErrorCode,
    ReadPdfResponse, ReadPdfSourceResult, SourceVisualOutcome, READ_PDF_ROUTE,
    VISUAL_NO_CANDIDATE_WARNING,
};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};

use crate::evidence::attach_evidence;
use crate::ocr_evidence::{run_read_ocr, ReadOcrOptions};
use crate::region_analysis_evidence;
use crate::schema::PdfSource;
use crate::visual_evidence::{materialize_read_source, MaterializedSource};

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

pub fn read_pdf(args: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let ocr_requested = args.get("include_ocr_text_layer").and_then(Value::as_bool) == Some(true);
    let sources = args
        .get("sources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let parsed_sources: Vec<PdfSource> = sources
        .iter()
        .enumerate()
        .map(|(source_index, source)| {
            serde_json::from_value(source.clone()).map_err(|error| {
                rmcp::ErrorData::invalid_params(
                    format!("Invalid sources[{source_index}]: {error}"),
                    None,
                )
            })
        })
        .collect::<Result<_, _>>()?;
    if parsed_sources.is_empty() {
        return Err(rmcp::ErrorData::invalid_params(
            "sources must include at least one PDF source.",
            None,
        ));
    }

    for source in &parsed_sources {
        if let Err(message) = source.validate() {
            return Err(rmcp::ErrorData::invalid_params(message, None));
        }
    }

    let mut materialized: Vec<Option<MaterializedSource>> =
        (0..parsed_sources.len()).map(|_| None).collect();
    let mut results = Vec::with_capacity(parsed_sources.len());
    for (source_index, source) in parsed_sources.iter().enumerate() {
        let original_label = source.label();
        let owner = match materialize_read_source(source_index, source) {
            Ok(owner) => owner,
            Err(error) => {
                results.push(ReadPdfSourceResult {
                    source: original_label,
                    success: false,
                    error: Some(error),
                    data: None,
                });
                continue;
            }
        };
        let mut source_args = args.clone();
        source_args["sources"] = json!([{
            "path": owner.path().to_string_lossy(),
            "pages": sources[source_index].get("pages").cloned().unwrap_or(Value::Null),
        }]);
        match read_pdf_from_value(&source_args) {
            Ok(mut response) => {
                let mut result = response.results.remove(0);
                result.source = original_label;
                results.push(result);
                materialized[source_index] = Some(owner);
            }
            Err(error) if error.code == ReadPdfErrorCode::InvalidParams => {
                return Err(rmcp::ErrorData::invalid_params(error.message, None));
            }
            Err(error) => {
                results.push(ReadPdfSourceResult {
                    source: original_label,
                    success: false,
                    error: Some(error.message),
                    data: None,
                });
            }
        }
    }
    if results.iter().all(|result| !result.success) {
        let errors = results
            .iter()
            .filter_map(|result| result.error.as_deref())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(rmcp::ErrorData::invalid_request(
            format!("All PDF sources failed to process: {errors}"),
            None,
        ));
    }

    let mut payload = ReadPdfResponse {
        profile: "pdf_read_results",
        results,
    };
    let ocr_sources = payload
        .results
        .iter()
        .enumerate()
        .filter_map(|(source_index, result)| {
            let pages = result.data.as_ref()?.ocr_candidate_pages.clone();
            (!pages.is_empty()).then(|| {
                materialized[source_index]
                    .as_ref()
                    .map(|source| (source, pages))
            })?
        })
        .collect::<Vec<_>>();
    let mut outcomes = if ocr_sources.is_empty() {
        Vec::new()
    } else {
        run_read_ocr(&ocr_sources, ReadOcrOptions::default())
    };
    if ocr_requested {
        outcomes.extend(
            payload
                .results
                .iter()
                .enumerate()
                .filter(|(_, result)| {
                    result
                        .data
                        .as_ref()
                        .is_some_and(|data| data.ocr_candidate_pages.is_empty())
                })
                .map(|(source_index, _)| anymd_core::SourceOcrOutcome {
                    source_index,
                    pages: Vec::new(),
                    warnings: Vec::new(),
                    error: None,
                }),
        );
    }
    fuse_ocr_outcomes(&mut payload, outcomes);

    let visual_requested = args
        .get("include_visual_enrichments")
        .and_then(Value::as_bool)
        == Some(true);
    let mut visual_outcomes: Vec<SourceVisualOutcome> = Vec::new();
    let mut has_provider_visual = false;
    if visual_requested {
        let provider_ready = region_analysis_evidence::command_provider_ready();
        if provider_ready {
            let max_visual = args
                .get("max_visual_enrichments")
                .and_then(Value::as_u64)
                .map(|value| value as u32)
                .unwrap_or(8)
                .clamp(1, 100);
            let mut analyze_sources = Vec::new();
            for (source_index, result) in payload.results.iter().enumerate() {
                let Some(data) = result.data.as_ref().filter(|_| result.success) else {
                    continue;
                };
                let candidates = data
                    .visual_enrichment_candidates
                    .as_ref()
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if candidates.is_empty() {
                    visual_outcomes.push(SourceVisualOutcome {
                        source_index,
                        provider_ready: true,
                        enrichments: Vec::new(),
                        warnings: vec![VISUAL_NO_CANDIDATE_WARNING.into()],
                        error: None,
                    });
                    continue;
                }
                let Some(owner) = materialized[source_index].as_ref() else {
                    visual_outcomes.push(SourceVisualOutcome {
                        source_index,
                        provider_ready: true,
                        enrichments: Vec::new(),
                        warnings: Vec::new(),
                        error: Some("Visual enrichment source materialization is missing.".into()),
                    });
                    continue;
                };
                analyze_sources.push((
                    source_index,
                    result.source.clone(),
                    owner.path().to_path_buf(),
                    candidates,
                ));
            }
            if !analyze_sources.is_empty() {
                visual_outcomes.extend(
                    region_analysis_evidence::analyze_visual_candidates_for_sources(
                        &analyze_sources,
                        max_visual,
                    ),
                );
            }
        }
        fuse_visual_outcomes(&mut payload, visual_outcomes);
        has_provider_visual = payload.results.iter().any(|result| {
            result
                .data
                .as_ref()
                .and_then(|data| data.visual_enrichments.as_ref())
                .and_then(Value::as_array)
                .is_some_and(|values| !values.is_empty())
        });
    }

    let source_hash = materialized
        .first()
        .and_then(Option::as_ref)
        .and_then(|source| hash_file(source.path(), DEFAULT_MAX_FILE_BYTES).ok())
        .map(|hash| hash.source_hash);

    let has_provider_ocr = payload.results.iter().any(|result| {
        result
            .data
            .as_ref()
            .and_then(|data| data.ocr_text_layer.as_ref())
            .is_some()
    });
    let public_payload = public_read_payload(&payload)?;
    let mut structured = attach_evidence(
        "read_pdf",
        None,
        &parsed_sources,
        READ_PDF_ROUTE,
        source_hash,
        Vec::new(),
        public_payload,
    );
    if has_provider_ocr || has_provider_visual {
        structured["evidence"]["confidence"] = json!("provider-dependent");
    }

    let mut result = CallToolResult::structured(structured);
    append_page_content(&mut result, &payload);
    append_ocr_content(&mut result, &payload);
    append_table_content(&mut result, &payload);
    Ok(result)
}

fn public_read_payload(payload: &ReadPdfResponse) -> Result<Value, rmcp::ErrorData> {
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

fn append_page_content(result: &mut CallToolResult, payload: &ReadPdfResponse) {
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

fn append_table_content(result: &mut CallToolResult, payload: &ReadPdfResponse) {
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

fn append_ocr_content(result: &mut CallToolResult, payload: &ReadPdfResponse) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn appends_nonempty_ocr_pages_after_existing_content() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
        let mut payload = read_pdf_from_value(&json!({
            "sources": [{"path": fixture}],
            "include_ocr_text_layer": true
        }))
        .expect("read fixture");
        payload.results[0]
            .data
            .as_mut()
            .expect("read data")
            .ocr_text_layer = Some(json!({
            "profile": "ocr_text_layer",
            "pages": [
                {"page": 2, "text": "scanned text"},
                {"page": 3, "text": ""}
            ]
        }));
        let mut result = CallToolResult::structured(json!({"profile":"pdf_read_results"}));
        result.content.push(ContentBlock::text("structured-json"));
        let existing_parts = result.content.len();

        append_ocr_content(&mut result, &payload);

        assert_eq!(result.content.len(), existing_parts + 1);
        let encoded = serde_json::to_value(result.content.last().expect("OCR content"))
            .expect("content json");
        assert_eq!(encoded["text"], "[Page 2 OCR]\nscanned text");
        assert!(result.structured_content.is_some());
    }

    #[test]
    fn public_table_projection_strips_rows_and_appends_separate_markdown() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
        let mut payload = read_pdf_from_value(&json!({
            "sources": [{"path": fixture}],
            "auto": false,
            "include_tables": true
        }))
        .expect("read fixture");
        payload.results[0].data.as_mut().expect("read data").tables = Some(json!([{
            "page": 1,
            "tableIndex": 0,
            "rows": [["Name", "Qty"], ["Apple", "2"]],
            "cells": [{"text":"Name"},{"text":"Qty"},{"text":"Apple"},{"text":"2"}],
            "rowCount": 2,
            "colCount": 2,
            "confidence": 0.95,
            "quality": {"signals":["complete_grid"]},
            "provenance": {"source":"selectable_text","engine":"pdf-reader-core"}
        }]));

        let public = public_read_payload(&payload).expect("public projection");
        let data = &public["results"][0]["data"];
        assert!(data.get("tables").is_none());
        assert_eq!(data["table_info"][0]["cellCount"], 4);
        assert!(data["table_info"][0].get("rows").is_none());
        assert!(data["table_info"][0].get("cells").is_none());

        let mut result = CallToolResult::structured(public);
        append_table_content(&mut result, &payload);
        let encoded = serde_json::to_value(result.content.last().expect("table content"))
            .expect("content json");
        let text = encoded["text"].as_str().expect("text content");
        assert!(text.starts_with("## Extracted Tables\n\n### Page 1, Table 1"));
        assert!(text.contains("| Name | Qty |\n| --- | --- |"));
    }

    #[test]
    fn public_image_projection_strips_binary_data_and_appends_png_parts_in_order() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
        let mut payload = read_pdf_from_value(&json!({
            "sources": [{"path": fixture}],
            "auto": false,
            "include_images": true
        }))
        .expect("read fixture");
        payload.results[0].data.as_mut().expect("read data").images = Some(json!([
            {"page":1,"index":0,"width":2,"height":2,"format":"grayscale","data":"cG5nLWE="},
            {"page":1,"index":1,"width":1,"height":1,"format":"rgb","data":"cG5nLWI="}
        ]));

        let public = public_read_payload(&payload).expect("public projection");
        let data = &public["results"][0]["data"];
        assert!(data.get("images").is_none());
        assert_eq!(
            data["image_info"],
            json!([
                {"page":1,"index":0,"width":2,"height":2,"format":"grayscale"},
                {"page":1,"index":1,"width":1,"height":1,"format":"rgb"}
            ])
        );
        assert!(data["image_info"][0].get("data").is_none());

        let mut result = CallToolResult::structured(public);
        append_page_content(&mut result, &payload);
        assert_eq!(result.content.len(), 3);
        let first = serde_json::to_value(&result.content[1]).expect("first image");
        let second = serde_json::to_value(&result.content[2]).expect("second image");
        assert_eq!(first["data"], "cG5nLWE=");
        assert_eq!(first["mimeType"], "image/png");
        assert_eq!(second["data"], "cG5nLWI=");
        assert_eq!(second["mimeType"], "image/png");
    }

    #[test]
    fn malformed_source_is_rejected_instead_of_shifting_source_ordinals() {
        let error = read_pdf(json!({
            "sources": [
                {"path": 42},
                {"path": "later.pdf"}
            ],
            "include_ocr_text_layer": true
        }))
        .expect_err("malformed first source must fail validation");
        assert!(error.message.contains("sources[0]"));
    }

    #[test]
    fn empty_selected_page_set_does_not_claim_or_warn_about_provider_execution() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
        let result = read_pdf(json!({
            "sources": [{"path": fixture, "pages": [999]}],
            "auto": false,
            "include_document_map": true,
            "include_ocr_text_layer": true
        }))
        .expect("read with invalid selected page");
        let structured = result.structured_content.expect("structured result");
        let data = &structured["results"][0]["data"];
        assert!(data.get("ocr_text_layer").is_none());
        assert_eq!(
            data["document_map"]["routing"]["needs_ocr_pages"],
            json!([])
        );
        assert_eq!(
            data["document_map"]["routing"]["ocr_applied_pages"],
            json!([])
        );
        assert_eq!(data["document_map"]["summary"]["ocr_page_count"], 0);
        assert_eq!(data["document_map"]["summary"]["ocr_text_chars"], 0);
        let warnings = data["warnings"].as_array().expect("warnings");
        assert!(warnings
            .iter()
            .any(|warning| { warning == "Requested page numbers 999 exceed total pages (1)." }));
        assert!(!warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|warning| warning.contains("provider"))));
    }
}
