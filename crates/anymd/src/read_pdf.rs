use anymd_core::{hash_file, READ_PDF_ROUTE};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::evidence::attach_evidence;

mod enrichment;
mod render;
mod sources;
#[cfg(test)]
mod tests;

use enrichment::{enrich_ocr, enrich_visual};
use render::{append_ocr_content, append_page_content, append_table_content, public_read_payload};
use sources::{read_sources, ReadSources};

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

pub fn read_pdf(args: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let ocr_requested = args.get("include_ocr_text_layer").and_then(Value::as_bool) == Some(true);
    let ReadSources {
        parsed_sources,
        materialized,
        mut payload,
    } = read_sources(&args)?;

    enrich_ocr(&mut payload, &materialized, ocr_requested);
    let has_provider_visual = enrich_visual(&mut payload, &materialized, &args);

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
