use std::path::PathBuf;

use anymd_core::read_pdf_from_value;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::json;

use super::read_pdf;
use super::render::{
    append_ocr_content, append_page_content, append_table_content, public_read_payload,
};

#[test]
fn appends_nonempty_ocr_pages_after_existing_content() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
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
    let encoded =
        serde_json::to_value(result.content.last().expect("OCR content")).expect("content json");
    assert_eq!(encoded["text"], "[Page 2 OCR]\nscanned text");
    assert!(result.structured_content.is_some());
}

#[test]
fn public_table_projection_strips_rows_and_appends_separate_markdown() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
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
    let encoded =
        serde_json::to_value(result.content.last().expect("table content")).expect("content json");
    let text = encoded["text"].as_str().expect("text content");
    assert!(text.starts_with("## Extracted Tables\n\n### Page 1, Table 1"));
    assert!(text.contains("| Name | Qty |\n| --- | --- |"));
}

#[test]
fn public_image_projection_strips_binary_data_and_appends_png_parts_in_order() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
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
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
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
