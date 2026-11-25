use anymd_core::hash_file;
use anymd_core::url_fetch::{cleanup_temp_file, fetch_url_to_temp_file};
use rmcp::model::CallToolResult;
use serde_json::{Value, json};
use std::path::PathBuf;

use crate::evidence::attach_evidence;
use crate::page_selection::selected_pages;
use crate::schema::PdfSource;

mod data;
mod sampling;

const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const INSPECT_ROUTE: &str = "rust-pdf-inspect-v1";

pub(super) fn inspect(args: Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let sources_value = args
        .get("sources")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| rmcp::ErrorData::invalid_params("sources is required", None))?;

    let parsed_sources: Vec<PdfSource> = sources_value
        .iter()
        .filter_map(|source| serde_json::from_value(source.clone()).ok())
        .collect();

    if parsed_sources.is_empty() {
        return Err(rmcp::ErrorData::invalid_params(
            "sources must include at least one PDF source",
            None,
        ));
    }

    for source in &parsed_sources {
        source
            .validate()
            .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
    }

    let mut results = Vec::new();
    let mut temps = Vec::new();
    for source in &parsed_sources {
        let targets = match selected_pages(&source.pages) {
            Ok(value) => value,
            Err(error) => {
                results.push(json!({
                    "source": source.label(),
                    "success": false,
                    "error": error,
                }));
                continue;
            }
        };
        let path_owned: PathBuf = if let Some(path) = source.path.as_ref() {
            PathBuf::from(path)
        } else if let Some(url) = source.url.as_ref() {
            match fetch_url_to_temp_file(url) {
                Ok(temp) => {
                    temps.push(temp.clone());
                    temp
                }
                Err(message) => {
                    results.push(json!({
                        "source": source.label(),
                        "success": false,
                        "error": message,
                    }));
                    continue;
                }
            }
        } else {
            results.push(json!({
                "source": source.label(),
                "success": false,
                "error": "Provide exactly one of path or url for each PDF source.",
            }));
            continue;
        };

        match data::inspect_source(path_owned.as_path(), targets.as_deref(), &args, source) {
            Ok(data) => results.push(json!({
                "source": source.label(),
                "success": true,
                "data": data,
            })),
            Err(error) => {
                results.push(json!({
                    "source": source.label(),
                    "success": false,
                    "error": error.message,
                }));
            }
        }
    }

    for temp in temps {
        cleanup_temp_file(temp.as_path());
    }

    let source_hash = parsed_sources
        .iter()
        .find_map(|source| source.path.as_ref())
        .and_then(|path| hash_file(PathBuf::from(path).as_path(), DEFAULT_MAX_FILE_BYTES).ok())
        .map(|hash| hash.source_hash);

    let structured = attach_evidence(
        "pdf_evidence",
        Some("inspect"),
        &parsed_sources,
        INSPECT_ROUTE,
        source_hash,
        Vec::new(),
        json!({ "results": results }),
    );

    Ok(CallToolResult::structured(structured))
}
