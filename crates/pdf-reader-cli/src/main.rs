use pdf_reader_core::read_pdf_from_value;
use pdf_reader_core::text_index::{extract_page_texts, search_pdf_text, TextIndexErrorCode};
use pdf_reader_core::{
    hash_file, search_pdf_from_value, ReadPdfErrorCode, SearchPdfErrorCode, ENGINE_NAME,
    ENGINE_VERSION, READ_PDF_ROUTE,
};
use serde::Deserialize;
use std::io::{self, Read};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Request {
    tool: String,
    input: serde_json::Value,
}

#[derive(Debug, serde::Serialize)]
struct HashSuccessEnvelope {
    status: &'static str,
    engine: &'static str,
    version: &'static str,
    hash: pdf_reader_core::FileHash,
}

#[derive(Debug, serde::Serialize)]
struct TextSearchSuccessEnvelope {
    status: &'static str,
    engine: &'static str,
    version: &'static str,
    search: pdf_reader_core::text_index::TextSearchResult,
}

#[derive(Debug, serde::Serialize)]
struct ErrorEnvelope {
    status: &'static str,
    code: String,
    message: String,
    next_action: String,
}

#[derive(Debug, serde::Serialize)]
struct ToolSuccessEnvelope {
    status: &'static str,
    engine: &'static str,
    version: &'static str,
    tool: String,
    result: serde_json::Value,
}

fn policy_code(code: pdf_reader_core::HashErrorCode) -> &'static str {
    match code {
        pdf_reader_core::HashErrorCode::InvalidParams => "INVALID_PARAMS",
        pdf_reader_core::HashErrorCode::InvalidRequest => "INVALID_REQUEST",
    }
}

fn text_index_error_code(code: TextIndexErrorCode) -> &'static str {
    match code {
        TextIndexErrorCode::InvalidParams => "INVALID_PARAMS",
        TextIndexErrorCode::InvalidRequest => "INVALID_REQUEST",
        TextIndexErrorCode::ExtractionFailed => "EXTRACTION_FAILED",
    }
}

fn read_pdf_error_code(code: ReadPdfErrorCode) -> &'static str {
    match code {
        ReadPdfErrorCode::InvalidParams => "INVALID_PARAMS",
        ReadPdfErrorCode::InvalidRequest => "INVALID_REQUEST",
        ReadPdfErrorCode::ExtractionFailed => "EXTRACTION_FAILED",
    }
}

fn search_pdf_error_code(code: SearchPdfErrorCode) -> &'static str {
    match code {
        SearchPdfErrorCode::InvalidParams => "INVALID_PARAMS",
        SearchPdfErrorCode::InvalidRequest => "INVALID_REQUEST",
        SearchPdfErrorCode::ExtractionFailed => "EXTRACTION_FAILED",
    }
}

fn wrap_tool_call_result(payload: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&payload).unwrap_or_else(|_| payload.to_string())
        }]
    })
}

fn handle_pdf_text_search(
    input: &serde_json::Value,
) -> Result<TextSearchSuccessEnvelope, ErrorEnvelope> {
    let path = input
        .get("path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| ErrorEnvelope {
            status: "error",
            code: "INVALID_PARAMS".into(),
            message: "path is required".into(),
            next_action: "Pass an absolute or cwd-relative PDF path.".into(),
        })?;

    let query = input
        .get("query")
        .and_then(|value| value.as_str())
        .ok_or_else(|| ErrorEnvelope {
            status: "error",
            code: "INVALID_PARAMS".into(),
            message: "query is required".into(),
            next_action: "Pass a non-empty search query.".into(),
        })?;

    let max_file_bytes = input
        .get("max_file_bytes")
        .and_then(|value| value.as_u64())
        .unwrap_or(256 * 1024 * 1024);

    let case_sensitive = input
        .get("case_sensitive")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let whole_word = input
        .get("whole_word")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let max_pages = input
        .get("max_pages")
        .and_then(|value| value.as_u64())
        .unwrap_or(100) as u32;

    let max_matches = input
        .get("max_matches")
        .and_then(|value| value.as_u64())
        .unwrap_or(50) as u32;

    let context_chars = input
        .get("context_chars")
        .and_then(|value| value.as_u64())
        .unwrap_or(120) as u32;

    match search_pdf_text(
        PathBuf::from(path).as_path(),
        max_file_bytes,
        query,
        case_sensitive,
        whole_word,
        max_pages,
        max_matches,
        context_chars,
    ) {
        Ok(search) => Ok(TextSearchSuccessEnvelope {
            status: "ok",
            engine: ENGINE_NAME,
            version: ENGINE_VERSION,
            search,
        }),
        Err(error) => Err(ErrorEnvelope {
            status: "error",
            code: text_index_error_code(error.code).into(),
            message: error.message,
            next_action: "Provide a readable PDF with extractable text.".into(),
        }),
    }
}

fn handle_pdf_hash(input: &serde_json::Value) -> Result<HashSuccessEnvelope, ErrorEnvelope> {
    let path = input
        .get("path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| ErrorEnvelope {
            status: "error",
            code: "INVALID_PARAMS".into(),
            message: "path is required".into(),
            next_action: "Pass an absolute or cwd-relative PDF path.".into(),
        })?;

    let max_file_bytes = input
        .get("max_file_bytes")
        .and_then(|value| value.as_u64())
        .unwrap_or(256 * 1024 * 1024);

    match hash_file(PathBuf::from(path).as_path(), max_file_bytes) {
        Ok(hash) => Ok(HashSuccessEnvelope {
            status: "ok",
            engine: ENGINE_NAME,
            version: ENGINE_VERSION,
            hash,
        }),
        Err(error) => Err(ErrorEnvelope {
            status: "error",
            code: policy_code(error.code).into(),
            message: error.message,
            next_action: "Provide a readable PDF file within configured safety limits.".into(),
        }),
    }
}

fn handle_search_pdf(input: &serde_json::Value) -> Result<ToolSuccessEnvelope, ErrorEnvelope> {
    let response = search_pdf_from_value(input).map_err(|error| ErrorEnvelope {
        status: "error",
        code: search_pdf_error_code(error.code).into(),
        message: error.message,
        next_action: "Provide readable local PDF sources and a non-empty query.".into(),
    })?;

    Ok(ToolSuccessEnvelope {
        status: "ok",
        engine: ENGINE_NAME,
        version: ENGINE_VERSION,
        tool: "search_pdf".into(),
        result: wrap_tool_call_result(serde_json::to_value(response).expect("serialize search")),
    })
}

fn handle_read_pdf(input: &serde_json::Value) -> Result<ToolSuccessEnvelope, ErrorEnvelope> {
    let response = read_pdf_from_value(input).map_err(|error| ErrorEnvelope {
        status: "error",
        code: read_pdf_error_code(error.code).into(),
        message: error.message,
        next_action: "Provide readable local PDF sources with valid read_pdf options.".into(),
    })?;

    Ok(ToolSuccessEnvelope {
        status: "ok",
        engine: ENGINE_NAME,
        version: ENGINE_VERSION,
        tool: "read_pdf".into(),
        result: wrap_tool_call_result(serde_json::to_value(response).expect("serialize read_pdf")),
    })
}

fn handle_pdf_compare(input: &serde_json::Value) -> Result<ToolSuccessEnvelope, ErrorEnvelope> {
    match pdf_reader_core::compare_pdf_from_value(input) {
        Ok(response) => Ok(ToolSuccessEnvelope {
            status: "ok",
            engine: pdf_reader_core::ENGINE_NAME,
            version: pdf_reader_core::ENGINE_VERSION,
            tool: "pdf_compare".into(),
            result: serde_json::to_value(response).expect("serialize pdf_compare"),
        }),
        Err(error) => Err(ErrorEnvelope {
            status: "error",
            code: "INVALID_PARAMS".into(),
            message: error.message,
            next_action: "Pass different local before/after PDF paths.".into(),
        }),
    }
}

fn handle_pdf_evidence(input: &serde_json::Value) -> Result<ToolSuccessEnvelope, ErrorEnvelope> {
    let operation = input
        .get("operation")
        .and_then(|value| value.as_str())
        .ok_or_else(|| ErrorEnvelope {
            status: "error",
            code: "INVALID_PARAMS".into(),
            message: "operation is required".into(),
            next_action: "Pass operation=inspect for the Rust inspect route.".into(),
        })?;

    if operation != "inspect" {
        return Err(ErrorEnvelope {
            status: "error",
            code: "UNSUPPORTED_OPERATION".into(),
            message: format!(
                "pdf-reader-cli supports operation=inspect only; operation={operation} belongs to the citra-mcp-server tool surface."
            ),
            next_action: "Use operation=inspect or call pdf_evidence through citra-mcp-server."
                .into(),
        });
    }

    let sources = input
        .get("sources")
        .and_then(|value| value.as_array())
        .ok_or_else(|| ErrorEnvelope {
            status: "error",
            code: "INVALID_PARAMS".into(),
            message: "sources is required".into(),
            next_action: "Pass sources with local PDF paths.".into(),
        })?;

    let mut results = Vec::new();
    for source in sources {
        let path = source
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ErrorEnvelope {
                status: "error",
                code: "INVALID_REQUEST".into(),
                message: "Rust pdf_evidence inspect requires local path sources.".into(),
                next_action: "Pass sources[].path.".into(),
            })?;

        let pages = extract_page_texts(PathBuf::from(path).as_path(), 256 * 1024 * 1024).map_err(
            |error| ErrorEnvelope {
                status: "error",
                code: text_index_error_code(error.code).into(),
                message: error.message,
                next_action: "Provide a readable PDF.".into(),
            },
        )?;

        let num_pages = pages.len().max(1) as u32;
        let text_chars: u32 = pages.iter().map(|page| page.chars().count() as u32).sum();
        results.push(serde_json::json!({
            "source": path,
            "success": true,
            "data": {
                "profile": "pdf_inspection",
                "num_pages": num_pages,
                "recommendation": {
                    "workflow": if text_chars > 80 { "text_extract" } else { "ocr_render" },
                    "needs_ocr": text_chars <= 80,
                    "route": "rust-pdf-inspect-v1",
                },
                "route": "rust-pdf-inspect-v1",
            }
        }));
    }

    let payload = serde_json::json!({
        "profile": "pdf_inspection_results",
        "operation": "inspect",
        "results": results,
        "route": READ_PDF_ROUTE,
    });

    Ok(ToolSuccessEnvelope {
        status: "ok",
        engine: ENGINE_NAME,
        version: ENGINE_VERSION,
        tool: "pdf_evidence".into(),
        result: wrap_tool_call_result(payload),
    })
}

fn main() {
    let mut payload = String::new();
    if io::stdin().read_to_string(&mut payload).is_err() {
        eprintln!("Failed to read stdin");
        std::process::exit(1);
    }

    let request: Request = match serde_json::from_str(&payload) {
        Ok(value) => value,
        Err(error) => {
            let envelope = ErrorEnvelope {
                status: "error",
                code: "INVALID_REQUEST".into(),
                message: format!("Invalid JSON request: {error}"),
                next_action: "Send {\"tool\":\"pdf_hash\",\"input\":{...}} on stdin.".into(),
            };
            println!("{}", serde_json::to_string(&envelope).expect("serialize"));
            std::process::exit(1);
        }
    };

    let output = match request.tool.as_str() {
        "pdf_hash" => match handle_pdf_hash(&request.input) {
            Ok(success) => serde_json::to_string(&success).expect("serialize"),
            Err(error) => serde_json::to_string(&error).expect("serialize"),
        },
        "pdf_text_search" => match handle_pdf_text_search(&request.input) {
            Ok(success) => serde_json::to_string(&success).expect("serialize"),
            Err(error) => serde_json::to_string(&error).expect("serialize"),
        },
        "read_pdf" => match handle_read_pdf(&request.input) {
            Ok(success) => serde_json::to_string(&success).expect("serialize"),
            Err(error) => serde_json::to_string(&error).expect("serialize"),
        },
        "pdf_compare" => match handle_pdf_compare(&request.input) {
            Ok(response) => serde_json::to_string(&response).expect("serialize"),
            Err(error) => serde_json::to_string(&error).expect("serialize error"),
        },
        "pdf_evidence" => match handle_pdf_evidence(&request.input) {
            Ok(success) => serde_json::to_string(&success).expect("serialize"),
            Err(error) => serde_json::to_string(&error).expect("serialize"),
        },
        "search_pdf" => match handle_search_pdf(&request.input) {
            Ok(success) => serde_json::to_string(&success).expect("serialize"),
            Err(error) => serde_json::to_string(&error).expect("serialize"),
        },
        other => serde_json::to_string(&ErrorEnvelope {
            status: "error",
            code: "UNSUPPORTED_TOOL".into(),
            message: format!("Unsupported tool: {other}"),
            next_action: "Use pdf_hash, pdf_text_search, read_pdf, search_pdf, or pdf_evidence."
                .into(),
        })
        .expect("serialize"),
    };

    println!("{output}");
}
