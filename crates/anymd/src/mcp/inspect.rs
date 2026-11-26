use rmcp::{handler::server::wrapper::Parameters, ErrorData};

use super::PdfReaderMcp;
use crate::document;
use crate::schema::{
    ComparePdfArgs, InspectArgs, InspectOperation, PdfEvidenceArgs, PdfEvidenceOperation,
    ReadPdfArgs,
};

impl PdfReaderMcp {
    pub(super) async fn run_inspect(
        &self,
        args: InspectArgs,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        let pro_feature = match args.operation {
            InspectOperation::VideoTimeline => Some("Video timeline"),
            InspectOperation::RenderFrame => Some("Video frame rendering"),
            InspectOperation::CiteCheck => Some("Cite-check"),
            _ => None,
        };
        if let Some(result) = pro_feature.and_then(|feature| self.pro_gate(feature)) {
            return Ok(result);
        }
        if matches!(
            args.operation,
            InspectOperation::VideoTimeline | InspectOperation::RenderFrame
        ) {
            let policy = self.source_access.clone();
            return tokio::task::spawn_blocking(move || {
                crate::video_request::inspect(args, &policy)
            })
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        }
        if args.timeline.is_some()
            || args.timestamps_ms.is_some()
            || (args.expected_source_sha256.is_some()
                && !matches!(args.operation, InspectOperation::CiteCheck))
            || args.transcript.is_some()
        {
            return Err(ErrorData::invalid_params(
                "video options require a video operation",
                None,
            ));
        }
        match args.operation {
            InspectOperation::CiteCheck => {
                crate::cite_check::inspect(args, self.source_access.clone()).await
            }
            InspectOperation::Compare => {
                let paths: Vec<String> = args
                    .sources
                    .iter()
                    .filter_map(|source| source.path.clone())
                    .collect();
                let [before, after] = paths.as_slice() else {
                    return Err(ErrorData::invalid_params(
                        "compare needs exactly two local PDF sources: [before, after].",
                        None,
                    ));
                };
                let before = self
                    .source_access
                    .admit_path(before)
                    .map_err(|message| ErrorData::invalid_params(message, None))?;
                let after = self
                    .source_access
                    .admit_path(after)
                    .map_err(|message| ErrorData::invalid_params(message, None))?;
                self.pdf_compare(Parameters(ComparePdfArgs {
                    before,
                    after,
                    max_file_bytes: None,
                    context_chars: None,
                }))
                .await
            }
            InspectOperation::Structure => {
                let profile = args.profile.clone().unwrap_or_else(|| "fast".into());
                if profile.eq_ignore_ascii_case("markdown") {
                    return Err(ErrorData::invalid_params(
                        "structure returns JSON; use read for Markdown.",
                        None,
                    ));
                }
                let sources: Vec<_> = args
                    .sources
                    .iter()
                    .map(|source| source.as_pdf_source())
                    .collect();
                let mut result = self
                    .read_pdf(Parameters(ReadPdfArgs {
                        sources: sources.clone(),
                        profile: Some(profile),
                        ..Default::default()
                    }))
                    .await?;
                let policy = self.source_access.clone();
                tokio::task::spawn_blocking(move || {
                    document::attach_structure_images(&mut result, &sources, &policy);
                    result
                })
                .await
                .map_err(|error| {
                    ErrorData::internal_error(format!("structure worker failed: {error}"), None)
                })
            }
            operation => {
                let operation = match operation {
                    InspectOperation::Inspect => PdfEvidenceOperation::Inspect,
                    InspectOperation::RenderPage => PdfEvidenceOperation::RenderPage,
                    InspectOperation::ExtractRegions => PdfEvidenceOperation::ExtractRegions,
                    InspectOperation::OcrPages => PdfEvidenceOperation::OcrPages,
                    InspectOperation::AnalyzeRegions => PdfEvidenceOperation::AnalyzeRegions,
                    InspectOperation::CiteCheck
                    | InspectOperation::Structure
                    | InspectOperation::Compare
                    | InspectOperation::VideoTimeline
                    | InspectOperation::RenderFrame => unreachable!(),
                };
                self.pdf_evidence(Parameters(PdfEvidenceArgs {
                    operation,
                    sources: args.sources,
                    sample_pages: args.sample_pages,
                    include_metadata: args.include_metadata,
                    scale: args.scale,
                    max_pages: args.max_pages,
                    max_regions: args.max_regions,
                    max_pixels_per_page: args.max_pixels_per_page,
                    include_image: args.include_image,
                    timeout_ms: args.timeout_ms,
                    max_output_chars: args.max_output_chars,
                    languages: args.languages,
                    ocr: args.ocr,
                }))
                .await
            }
        }
    }
}
