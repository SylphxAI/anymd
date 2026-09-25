//! Real-document regression for the Markdown layout engine.
//!
//! Runs when ANYMD_CORPUS_DIR points at a directory filled by
//! scripts/fetch-markdown-corpus.sh; skips otherwise.

use anymd_core::markdown_layout::{load_document, pdf_to_markdown};
use serde_json::Value;
use std::path::PathBuf;

#[test]
fn corpus_documents_convert_to_expected_markdown() {
    let Ok(dir) = std::env::var("ANYMD_CORPUS_DIR") else {
        eprintln!("ANYMD_CORPUS_DIR not set; skipping markdown corpus regression");
        return;
    };
    let manifest_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/markdown-regression.json");
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path).expect("manifest"))
            .expect("json");
    let mut failures = Vec::new();
    for case in manifest["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().unwrap();
        let path = PathBuf::from(&dir).join(case["file"].as_str().unwrap());
        let doc = load_document(&path).unwrap_or_else(|e| panic!("{id}: {}", e.message));
        let converted =
            pdf_to_markdown(&doc, None).unwrap_or_else(|e| panic!("{id}: {}", e.message));
        let markdown: String = converted
            .pages
            .iter()
            .map(|page| page.markdown.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        for needle in case["contains"].as_array().unwrap() {
            let needle = needle.as_str().unwrap();
            if !markdown.contains(needle) {
                failures.push(format!("{id}: missing {needle:?}"));
            }
        }
        for needle in case["absent"].as_array().unwrap() {
            let needle = needle.as_str().unwrap();
            if markdown.contains(needle) {
                failures.push(format!("{id}: unexpected {needle:?}"));
            }
        }
        if let Some(ordered) = case.get("ordered").and_then(Value::as_array) {
            let mut from = 0;
            for needle in ordered {
                let needle = needle.as_str().unwrap();
                match markdown[from..].find(needle) {
                    Some(index) => from += index + needle.len(),
                    None => failures.push(format!("{id}: {needle:?} missing or out of order")),
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "markdown corpus regressions:\n{}",
        failures.join("\n")
    );
}
