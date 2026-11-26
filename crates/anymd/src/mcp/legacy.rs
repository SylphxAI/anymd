use rmcp::{handler::server::wrapper::Parameters, ErrorData};
use serde_json::Value;

use super::PdfReaderMcp;
use crate::schema::{
    ComparePdfArgs, PdfEvidenceArgs, PdfEvidenceOperation, ReadPdfArgs, SearchPdfArgs,
};
use crate::schema_sanitize::omit_absent_optional_fields;
use crate::{lean, pdf_compare, pdf_evidence, read_pdf, search};

/// Legacy tool names that stay callable (unlisted) for one major version.
pub const LEGACY_TOOL_NAMES: &[&str] = &["read_pdf", "search_pdf", "pdf_evidence", "pdf_compare"];

fn legacy_args<T: serde::de::DeserializeOwned>(
    request: &rmcp::model::CallToolRequestParams,
) -> Result<T, ErrorData> {
    let value = Value::Object(request.arguments.clone().unwrap_or_default());
    serde_json::from_value(value).map_err(|error| {
        ErrorData::invalid_params(
            format!("Invalid arguments for {}: {error}", request.name),
            None,
        )
    })
}

/// Legacy tool names, still callable (not listed) for one major version.
impl PdfReaderMcp {
    pub async fn read_pdf(
        &self,
        Parameters(mut args): Parameters<ReadPdfArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        args.validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        self.source_access
            .admit_pdf_sources(&mut args.sources)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        if !lean::read_wants_legacy(&args) {
            let policy = self.source_access.clone();
            return tokio::task::spawn_blocking(move || lean::read_pdf(&args, &policy))
                .await
                .map_err(|error| {
                    ErrorData::internal_error(format!("read_pdf worker failed: {error}"), None)
                })?;
        }
        args.max_tokens = None;
        args.cursor = None;
        let provider_operation = args.include_ocr_text_layer == Some(true);
        let value = serde_json::to_value(args)
            .map(omit_absent_optional_fields)
            .map_err(|error| {
                ErrorData::invalid_params(format!("Failed to encode read_pdf args: {error}"), None)
            })?;
        if provider_operation {
            tokio::task::spawn_blocking(move || read_pdf::read_pdf(value))
                .await
                .map_err(|error| {
                    ErrorData::internal_error(format!("read_pdf worker failed: {error}"), None)
                })?
        } else {
            read_pdf::read_pdf(value)
        }
    }

    pub async fn pdf_compare(
        &self,
        Parameters(args): Parameters<ComparePdfArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        args.validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        self.source_access
            .admit_path(&args.before)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        self.source_access
            .admit_path(&args.after)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let value = serde_json::to_value(args).map_err(|error| {
            ErrorData::invalid_params(format!("Failed to encode pdf_compare args: {error}"), None)
        })?;
        pdf_compare::pdf_compare(value)
    }

    pub async fn search_pdf(
        &self,
        Parameters(mut args): Parameters<SearchPdfArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        args.validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        self.source_access
            .admit_pdf_sources(&mut args.sources)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        if !lean::search_wants_legacy(&args) {
            let policy = self.source_access.clone();
            return tokio::task::spawn_blocking(move || lean::search_pdf(&args, &policy))
                .await
                .map_err(|error| {
                    ErrorData::internal_error(format!("search_pdf worker failed: {error}"), None)
                })?;
        }
        args.detail = None;
        let provider_operation = args.include_ocr_text_layer == Some(true);
        let value = serde_json::to_value(args)
            .map(omit_absent_optional_fields)
            .map_err(|error| {
                ErrorData::invalid_params(
                    format!("Failed to encode search_pdf args: {error}"),
                    None,
                )
            })?;
        if provider_operation {
            tokio::task::spawn_blocking(move || search::search_pdf(value))
                .await
                .map_err(|error| {
                    ErrorData::internal_error(format!("search_pdf worker failed: {error}"), None)
                })?
        } else {
            search::search_pdf(value)
        }
    }

    pub async fn pdf_evidence(
        &self,
        Parameters(mut args): Parameters<PdfEvidenceArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        args.validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        self.source_access
            .admit_evidence_sources(&mut args.sources)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let provider_operation = matches!(
            args.operation,
            PdfEvidenceOperation::OcrPages | PdfEvidenceOperation::AnalyzeRegions
        );
        let value = serde_json::to_value(args)
            .map(omit_absent_optional_fields)
            .map_err(|error| {
                ErrorData::invalid_params(
                    format!("Failed to encode pdf_evidence args: {error}"),
                    None,
                )
            })?;
        if provider_operation {
            tokio::task::spawn_blocking(move || pdf_evidence::pdf_evidence(value))
                .await
                .map_err(|error| {
                    ErrorData::internal_error(format!("Provider worker failed: {error}"), None)
                })?
        } else {
            pdf_evidence::pdf_evidence(value)
        }
    }

    pub(crate) async fn call_legacy(
        &self,
        request: &rmcp::model::CallToolRequestParams,
    ) -> Option<Result<rmcp::model::CallToolResult, ErrorData>> {
        Some(match request.name.as_ref() {
            "read_pdf" => match legacy_args(request) {
                Ok(args) => self.read_pdf(Parameters(args)).await,
                Err(error) => Err(error),
            },
            "search_pdf" => match legacy_args(request) {
                Ok(args) => self.search_pdf(Parameters(args)).await,
                Err(error) => Err(error),
            },
            "pdf_evidence" => match legacy_args(request) {
                Ok(args) => self.pdf_evidence(Parameters(args)).await,
                Err(error) => Err(error),
            },
            "pdf_compare" => match legacy_args(request) {
                Ok(args) => self.pdf_compare(Parameters(args)).await,
                Err(error) => Err(error),
            },
            _ => return None,
        })
    }
}
