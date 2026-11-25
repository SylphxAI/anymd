use anymd_core::{read_pdf_from_value, ReadPdfErrorCode, ReadPdfResponse, ReadPdfSourceResult};
use serde_json::{json, Value};

use crate::schema::PdfSource;
use crate::visual_evidence::{materialize_read_source, MaterializedSource};

pub(super) struct ReadSources {
    pub(super) parsed_sources: Vec<PdfSource>,
    pub(super) materialized: Vec<Option<MaterializedSource>>,
    pub(super) payload: ReadPdfResponse,
}

pub(super) fn read_sources(args: &Value) -> Result<ReadSources, rmcp::ErrorData> {
    let sources = args
        .get("sources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let parsed_sources: Vec<PdfSource> = sources
        .iter()
        .enumerate()
        .map(|(source_index, source)| {
            serde_json::from_value(source.clone()).map_err(|error| {
                rmcp::ErrorData::invalid_params(
                    format!("Invalid sources[{source_index}]: {error}"),
                    None,
                )
            })
        })
        .collect::<Result<_, _>>()?;
    if parsed_sources.is_empty() {
        return Err(rmcp::ErrorData::invalid_params(
            "sources must include at least one PDF source.",
            None,
        ));
    }

    for source in &parsed_sources {
        if let Err(message) = source.validate() {
            return Err(rmcp::ErrorData::invalid_params(message, None));
        }
    }

    let mut materialized: Vec<Option<MaterializedSource>> =
        (0..parsed_sources.len()).map(|_| None).collect();
    let mut results = Vec::with_capacity(parsed_sources.len());
    for (source_index, source) in parsed_sources.iter().enumerate() {
        let original_label = source.label();
        let owner = match materialize_read_source(source_index, source) {
            Ok(owner) => owner,
            Err(error) => {
                results.push(ReadPdfSourceResult {
                    source: original_label,
                    success: false,
                    error: Some(error),
                    data: None,
                });
                continue;
            }
        };
        let mut source_args = args.clone();
        source_args["sources"] = json!([{
            "path": owner.path().to_string_lossy(),
            "pages": sources[source_index].get("pages").cloned().unwrap_or(Value::Null),
        }]);
        match read_pdf_from_value(&source_args) {
            Ok(mut response) => {
                let mut result = response.results.remove(0);
                result.source = original_label;
                results.push(result);
                materialized[source_index] = Some(owner);
            }
            Err(error) if error.code == ReadPdfErrorCode::InvalidParams => {
                return Err(rmcp::ErrorData::invalid_params(error.message, None));
            }
            Err(error) => {
                results.push(ReadPdfSourceResult {
                    source: original_label,
                    success: false,
                    error: Some(error.message),
                    data: None,
                });
            }
        }
    }
    if results.iter().all(|result| !result.success) {
        let errors = results
            .iter()
            .filter_map(|result| result.error.as_deref())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(rmcp::ErrorData::invalid_request(
            format!("All PDF sources failed to process: {errors}"),
            None,
        ));
    }

    let payload = ReadPdfResponse {
        profile: "pdf_read_results",
        results,
    };
    Ok(ReadSources {
        parsed_sources,
        materialized,
        payload,
    })
}
