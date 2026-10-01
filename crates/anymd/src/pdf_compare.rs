use rmcp::model::CallToolResult;
use serde_json::Value;

pub fn pdf_compare(args: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let response = anymd_core::compare_pdf_from_value(&args).map_err(|error| {
        rmcp::ErrorData::invalid_params(error.message, None)
    })?;
    Ok(CallToolResult::structured(serde_json::to_value(response).map_err(|error| {
        rmcp::ErrorData::internal_error(format!("Failed to serialize pdf_compare: {error}"), None)
    })?))
}
