use anymd_core::{text_index::cite_check::CiteDecision, OcrPage};
use rmcp::{model::CallToolResult, ErrorData};
use serde_json::{json, Value};

use crate::schema::{Citation, InspectArgs};

const SCOPE: &str = "Quote/location support in extracted evidence; not semantic truth or independent confirmation of OCR accuracy.";

pub(super) fn native_layer() -> Value {
    json!({"kind": "native_text"})
}

pub(super) fn ocr_layer(page: &OcrPage) -> Value {
    json!({"kind": "ocr", "provider": page.provider, "provenance": page.provenance, "render_evidence_id": page.source_render_evidence_id})
}

pub(super) fn citation_result(
    citation: &Citation,
    hash: &str,
    decision: CiteDecision,
    layer: Value,
) -> Value {
    json!({"id": citation.id, "page": citation.page, "source_hash": hash, "verdict": decision.verdict, "evidence_layer": layer, "locations": decision.locations})
}

pub(super) fn evaluated(args: &InspectArgs, hash: &str, results: Vec<Value>) -> Value {
    json!({"scope": SCOPE, "source_hash": hash, "normalization": args.normalization.unwrap_or_default(), "results": results})
}

pub(super) fn insufficient(args: &InspectArgs, reason: &str, hash: Option<&str>) -> Value {
    json!({"scope": SCOPE, "source_hash": hash, "results": args.citations.as_deref().unwrap_or_default().iter().map(|c| json!({"id": c.id, "page": c.page, "verdict": "insufficient_evidence", "reason": reason, "locations": []})).collect::<Vec<_>>()})
}

pub(super) fn serialize(
    mut output: Value,
    args: &InspectArgs,
    hash: &str,
) -> Result<String, String> {
    let maximum = args.max_output_chars.unwrap_or(200000) as usize;
    // Never truncate text/geometry and leave a verified label attached.
    if output.to_string().encode_utf16().count() > maximum {
        output = insufficient(
            args,
            "Citation output exceeds max_output_chars; supporting geometry was not returned.",
            Some(hash),
        );
    }
    let serialized = output.to_string();
    if serialized.encode_utf16().count() > maximum {
        return Err("Citation result envelope exceeds max_output_chars.".into());
    }
    Ok(serialized)
}

pub(super) fn from_worker(
    result: Result<String, String>,
    args: &InspectArgs,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(text) => {
            let value: Value = serde_json::from_str(&text).map_err(|e| {
                ErrorData::internal_error(format!("Invalid cite-check worker output: {e}"), None)
            })?;
            Ok(CallToolResult::structured(value))
        }
        Err(reason) if reason.contains("deadline") => {
            let value = insufficient(args, &reason, None);
            if value.to_string().encode_utf16().count() > maximum {
                Ok(CallToolResult::error(vec![
                    rmcp::model::ContentBlock::text(
                        "Citation result envelope exceeds max_output_chars.",
                    ),
                ]))
            } else {
                Ok(CallToolResult::structured(value))
            }
        }
        Err(reason) => Ok(CallToolResult::error(vec![
            rmcp::model::ContentBlock::text(reason),
        ])),
    }
}
