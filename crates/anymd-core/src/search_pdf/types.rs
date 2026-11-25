use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::text_index::{TextIndexError, TextIndexErrorCode, TEXT_INDEX_ROUTE};

pub const SEARCH_PDF_ROUTE: &str = TEXT_INDEX_ROUTE;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SearchPdfSource {
    pub path: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub pages: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SearchPdfInput {
    pub sources: Vec<SearchPdfSource>,
    pub query: String,
    pub case_sensitive: Option<bool>,
    pub whole_word: Option<bool>,
    pub include_ocr_text_layer: Option<bool>,
    pub max_pages: Option<u32>,
    pub max_matches_per_source: Option<u32>,
    pub context_chars: Option<u32>,
    pub prefer_speed: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchPdfResponse {
    pub profile: &'static str,
    pub search_options: Value,
    pub results: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchPdfErrorCode {
    InvalidParams,
    InvalidRequest,
    ExtractionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPdfError {
    pub code: SearchPdfErrorCode,
    pub message: String,
}

impl SearchPdfError {
    pub(super) fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: SearchPdfErrorCode::InvalidParams,
            message: message.into(),
        }
    }

    pub(super) fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: SearchPdfErrorCode::InvalidRequest,
            message: message.into(),
        }
    }
}

impl crate::pages::PageSpecError for SearchPdfError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self::invalid_params(message)
    }
}

impl From<TextIndexError> for SearchPdfError {
    fn from(error: TextIndexError) -> Self {
        match error.code {
            TextIndexErrorCode::InvalidParams => Self::invalid_params(error.message),
            TextIndexErrorCode::InvalidRequest => Self::invalid_request(error.message),
            TextIndexErrorCode::ExtractionFailed => Self {
                code: SearchPdfErrorCode::ExtractionFailed,
                message: error.message,
            },
        }
    }
}
