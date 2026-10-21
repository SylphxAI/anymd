//! The pieces `pdf_evidence`'s OCR and region-analysis providers both use: the
//! tool's argument parsing, the UTF-16 truncation of provider text, and the
//! JSON-to-tool-result wrapper. One statement each, so the two providers answer
//! with the same errors and the same text limits.

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::Value;

use crate::schema::PdfEvidenceArgs;

/// `pdf_evidence` arguments as the tool's own validation accepts them.
pub(crate) fn parse_pdf_evidence_args(value: &Value) -> Result<PdfEvidenceArgs, rmcp::ErrorData> {
    let args: PdfEvidenceArgs = serde_json::from_value(value.clone()).map_err(|error| {
        rmcp::ErrorData::invalid_params(format!("Invalid pdf_evidence arguments: {error}"), None)
    })?;
    args.validate()
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    Ok(args)
}

/// `text` cut to at most `maximum` UTF-16 code units, and whether it was cut.
/// A surrogate pair is never split.
pub(crate) fn truncate_utf16(text: &str, maximum: usize) -> (String, bool) {
    let mut units = 0usize;
    let mut end = text.len();
    for (index, character) in text.char_indices() {
        let next = units + character.len_utf16();
        if next > maximum {
            end = index;
            break;
        }
        units = next;
    }
    (
        text[..end].to_string(),
        text.encode_utf16().count() > maximum,
    )
}

/// A provider answer as one pretty-printed JSON text block.
pub(crate) fn json_result(payload: Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
    let mut result = CallToolResult::structured(payload);
    result.content = vec![ContentBlock::text(text)];
    result
}

/// A tool-level failure the caller reads as an error result, not a protocol error.
pub(crate) fn error_result(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn truncation_never_splits_a_surrogate_pair_and_reports_the_cut() {
        assert_eq!(truncate_utf16("abcd", 4), ("abcd".to_string(), false));
        assert_eq!(truncate_utf16("abcde", 4), ("abcd".to_string(), true));
        assert_eq!(truncate_utf16("a😀b", 2), ("a".to_string(), true));
        assert_eq!(truncate_utf16("a😀b", 4), ("a😀b".to_string(), false));
    }

    #[test]
    fn a_json_result_carries_the_structured_value_and_one_text_block() {
        let value = json!({"ok": true});
        let result = json_result(value.clone());
        assert_eq!(result.structured_content.as_ref(), Some(&value));
    }
}
