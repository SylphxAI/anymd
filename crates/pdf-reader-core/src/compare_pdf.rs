//! Bounded text-level PDF comparison for document review workflows.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::text_index::{extract_page_texts, TextIndexError};

#[derive(Debug, Clone, Deserialize)]
pub struct ComparePdfInput {
    pub before: String,
    pub after: String,
    #[serde(default)]
    pub max_file_bytes: Option<u64>,
    #[serde(default)]
    pub context_chars: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComparePdfResponse {
    pub profile: &'static str,
    pub identical: bool,
    pub before_pages: usize,
    pub after_pages: usize,
    pub changed_pages: Vec<u32>,
    pub added_terms: Vec<String>,
    pub removed_terms: Vec<String>,
    pub before_chars: usize,
    pub after_chars: usize,
    pub route: &'static str,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComparePdfErrorCode {
    InvalidParams,
    InvalidRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparePdfError {
    pub code: ComparePdfErrorCode,
    pub message: String,
}

impl ComparePdfError {
    fn invalid_params(message: impl Into<String>) -> Self { Self { code: ComparePdfErrorCode::InvalidParams, message: message.into() } }
    fn invalid_request(message: impl Into<String>) -> Self { Self { code: ComparePdfErrorCode::InvalidRequest, message: message.into() } }
}

impl From<TextIndexError> for ComparePdfError {
    fn from(error: TextIndexError) -> Self {
        match error.code {
            crate::text_index::TextIndexErrorCode::InvalidParams => Self::invalid_params(error.message),
            crate::text_index::TextIndexErrorCode::InvalidRequest => Self::invalid_request(error.message),
            crate::text_index::TextIndexErrorCode::ExtractionFailed => Self::invalid_request(error.message),
        }
    }
}

fn terms(text: &str) -> std::collections::HashSet<String> {
    text.split_whitespace()
        .map(|token| token.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

pub fn compare_pdf(input: &ComparePdfInput) -> Result<ComparePdfResponse, ComparePdfError> {
    if input.before.trim().is_empty() || input.after.trim().is_empty() {
        return Err(ComparePdfError::invalid_params("before and after are required local PDF paths"));
    }
    if input.before == input.after {
        return Err(ComparePdfError::invalid_params("before and after must be different PDFs"));
    }
    if input.context_chars.is_some_and(|value| value > 1000) {
        return Err(ComparePdfError::invalid_params("context_chars must be <= 1000"));
    }
    let max_file_bytes = input.max_file_bytes.unwrap_or(256 * 1024 * 1024);
    let before_pages = extract_page_texts(&PathBuf::from(&input.before), max_file_bytes)?;
    let after_pages = extract_page_texts(&PathBuf::from(&input.after), max_file_bytes)?;
    let before_text = before_pages.join("\n");
    let after_text = after_pages.join("\n");
    let before_terms = terms(&before_text);
    let after_terms = terms(&after_text);
    let mut added_terms: Vec<String> = after_terms.difference(&before_terms).cloned().collect();
    let mut removed_terms: Vec<String> = before_terms.difference(&after_terms).cloned().collect();
    added_terms.sort();
    removed_terms.sort();
    let mut changed_pages = Vec::new();
    for index in 0..before_pages.len().max(after_pages.len()) {
        let before = before_pages.get(index).map(String::as_str).unwrap_or("");
        let after = after_pages.get(index).map(String::as_str).unwrap_or("");
        if before != after {
            changed_pages.push(index as u32 + 1);
        }
    }
    Ok(ComparePdfResponse {
        profile: "pdf_compare_results",
        identical: changed_pages.is_empty() && before_text == after_text,
        before_pages: before_pages.len(),
        after_pages: after_pages.len(),
        changed_pages,
        added_terms: added_terms.into_iter().take(200).collect(),
        removed_terms: removed_terms.into_iter().take(200).collect(),
        before_chars: before_text.chars().count(),
        after_chars: after_text.chars().count(),
        route: "rust-text-compare",
        warnings: vec!["Comparison is text-level; visual-only layout changes are not claimed.".into()],
    })
}

pub fn compare_pdf_from_value(input: &Value) -> Result<ComparePdfResponse, ComparePdfError> {
    let parsed: ComparePdfInput = serde_json::from_value(input.clone())
        .map_err(|error| ComparePdfError::invalid_params(format!("Invalid pdf_compare input: {error}")))?;
    compare_pdf(&parsed)
}
