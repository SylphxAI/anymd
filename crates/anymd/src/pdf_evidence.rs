use rmcp::model::CallToolResult;
use serde_json::Value;

use crate::{ocr_evidence, region_analysis_evidence, visual_evidence};

mod inspect;

#[cfg(test)]
mod tests;

pub fn pdf_evidence(args: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| rmcp::ErrorData::invalid_params("operation is required", None))?;

    match operation {
        "inspect" => inspect::inspect(args),
        "render_page" => visual_evidence::render_pages(args),
        "extract_regions" => visual_evidence::extract_regions(args),
        "ocr_pages" => ocr_evidence::ocr_pages(args),
        "analyze_regions" => region_analysis_evidence::analyze_regions(args),
        other => Err(rmcp::ErrorData::invalid_params(
            format!("Unsupported pdf_evidence operation: {other}"),
            None,
        )),
    }
}
