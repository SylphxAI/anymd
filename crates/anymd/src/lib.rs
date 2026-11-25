pub mod cite_check;
pub mod cli;
mod command_provider;
pub mod discover_compat;
pub mod document;
pub mod evidence;
pub mod http_transport;
pub mod lean;
mod ocr_evidence;
pub mod ocr_vlm;
pub mod outline;
mod page_selection;
pub mod pdf_compare;
pub mod pdf_evidence;
mod pdf_evidence_common;
pub mod pro;
mod provider_budget;
pub mod read_pdf;
mod region_analysis_evidence;
pub mod schema;
mod schema_sanitize;
pub mod search;
pub mod setup;
pub mod source_access;
pub mod video_evidence;
pub mod video_request;
mod visual_evidence;

use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::tool::ToolCallContext,
    handler::server::wrapper::Parameters,
    model::{
        CacheScope, CallToolResponse, CustomRequest, CustomResult, Implementation, ListToolsResult,
        MetaObject, PaginatedRequestParams, ProtocolVersion, ResultType, ServerCapabilities,
        ServerConfig,
    },
    service::{RequestContext, RoleServer},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};

use crate::schema::{
    ComparePdfArgs, InspectArgs, InspectOperation, OutlineArgs, PdfEvidenceArgs,
    PdfEvidenceOperation, ReadArgs, ReadPdfArgs, SearchArgs, SearchPdfArgs,
};
use crate::schema_sanitize::{omit_absent_optional_fields, sanitized_tools};
use crate::source_access::SourceAccessPolicy;
use serde_json::Value;

pub const SERVER_NAME: &str = "anymd";
/// The product version: the workspace version, set with `bun scripts/set-version.ts`.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// GitHub release assets, `<base>/v<version>/<asset>`: the VLM OCR companion
/// and its checksum manifest.
pub const RELEASE_DOWNLOAD_BASE: &str = "https://github.com/SylphxAI/anymd/releases/download";
pub const SERVER_INFO_META_KEY: &str = "io.modelcontextprotocol/serverInfo";
pub const SERVER_INSTRUCTIONS: &str =
    "Local document reader for agents. read turns any file, URL, or directory listing into clean \
Markdown (PDF, Office, EPUB, HTML, CSV, images, media) with page/slide/sheet markers and a cursor \
for long documents. outline returns a document tree; read node fetches a section. search finds text across files and directories with page locators and node title paths. inspect \
renders, crops, OCRs, diffs, or returns structured JSON for PDFs. No cloud API key is required. \
anymd Pro ($29 once, `anymd pro buy` or https://anymd.sylphx.com/pricing) adds inspect cite_check (accepts read/search output directly) and video_timeline / render_frame for verifiable video evidence.";

#[derive(Clone)]
pub struct PdfReaderMcp {
    pub tool_router: ToolRouter<Self>,
    source_access: SourceAccessPolicy,
    pro_active: fn() -> bool,
}

impl PdfReaderMcp {
    /// Replace the licence check (tests inject a fixed answer).
    pub fn with_pro_check(mut self, pro_active: fn() -> bool) -> Self {
        self.pro_active = pro_active;
        self
    }

    /// Gate for Pro-only operations: `Some(result)` is the normal tool result
    /// that carries the ProRequired message; nothing has been opened or run.
    fn pro_gate(&self, feature: &str) -> Option<rmcp::model::CallToolResult> {
        pro::require_pro_with(feature, (self.pro_active)())
            .err()
            .map(|required| pro::required_result(&required))
    }

    pub fn new() -> Self {
        Self::with_source_access(SourceAccessPolicy::unrestricted())
    }

    pub fn with_source_access(source_access: SourceAccessPolicy) -> Self {
        Self {
            tool_router: Self::tool_router(),
            source_access,
            pro_active: || pro::current_license().is_some(),
        }
    }
}

impl Default for PdfReaderMcp {
    fn default() -> Self {
        Self::new()
    }
}

fn server_result_meta(implementation: &Implementation) -> MetaObject {
    let mut meta = MetaObject::new();
    meta.insert(
        SERVER_INFO_META_KEY.to_string(),
        serde_json::to_value(implementation).expect("Implementation serialization cannot fail"),
    );
    meta
}

fn uses_2026_envelope(context: &RequestContext<RoleServer>) -> bool {
    context
        .protocol_version()
        .is_some_and(|version| version >= ProtocolVersion::V_2026_07_28)
}

#[tool_router]
impl PdfReaderMcp {
    #[tool(
        description = "Read any document as clean Markdown: PDF, Word (DOCX), PowerPoint (PPTX), Excel (XLSX/XLS/ODS), CSV, EPUB, HTML or a web URL, Markdown/text, images (metadata + OCR), audio/video (metadata, chapters, subtitles). Pages/slides/sheets carry <!-- page N --> style markers for citation. Long documents stop at max_tokens (default 20000) and end with a cursor to continue; choose pages with pages: \"1-5,8\". A directory returns its readable files."
    )]
    pub async fn read(
        &self,
        Parameters(args): Parameters<ReadArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        args.validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        if args.timeline.is_some() {
            if let Some(result) = self.pro_gate("Video timeline") {
                return Ok(result);
            }
        }
        let policy = self.source_access.clone();
        tokio::task::spawn_blocking(move || lean::read(&args, &policy))
            .await
            .map_err(|error| {
                ErrorData::internal_error(format!("read worker failed: {error}"), None)
            })?
    }

    #[tool(
        description = "Search documents for text: one file, many files, whole directories (recursive, .gitignore aware), or URLs, across every format read supports. Returns each hit as file + page/slide/sheet + a snippet with the match in bold. mode auto (default) finds the exact phrase and falls back to BM25-ranked passages when there is none; literal or ranked force one. Narrow directories with glob, e.g. \"*.pdf\"."
    )]
    pub async fn search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        args.validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let policy = self.source_access.clone();
        tokio::task::spawn_blocking(move || lean::search(&args, &policy))
            .await
            .map_err(|error| {
                ErrorData::internal_error(format!("search worker failed: {error}"), None)
            })?
    }

    #[tool(
        description = "Return a document heading tree with stable node ids, title paths, page/slide/chapter ranges and Markdown byte ranges. PDF bookmarks or detected headings, Word headings, PowerPoint slides, EPUB chapters and headings, HTML and Markdown. format json (default) or tree. Pass a node id to read to fetch that section."
    )]
    pub async fn outline(
        &self,
        Parameters(args): Parameters<OutlineArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        if args.timeline.is_some() {
            if let Some(result) = self.pro_gate("Video timeline") {
                return Ok(result);
            }
        }
        let policy = self.source_access.clone();
        tokio::task::spawn_blocking(move || outline::tool(&args, &policy))
            .await
            .map_err(|error| {
                ErrorData::internal_error(format!("outline worker failed: {error}"), None)
            })?
    }

    #[tool(
        description = "Document and media evidence when Markdown is not enough. video_timeline (bounded scene/cue timeline) and render_frame (actual decoded frames), and cite_check (quote/location support, not semantic truth), are anymd Pro; PDF operations: inspect (page facts, metadata), render_page (PNG images), extract_regions (crop bounding boxes), ocr_pages / analyze_regions (configured OCR or vision provider), structure (JSON with document map, elements, geometry; profile quality|research adds trust and accessibility reports), compare (page-level diff of sources[0] vs sources[1])."
    )]
    pub async fn inspect(
        &self,
        Parameters(args): Parameters<InspectArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        self.run_inspect(args).await
    }
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
}

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

impl PdfReaderMcp {
    async fn run_inspect(
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

    async fn call_legacy(
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

#[tool_handler]
impl ServerHandler for PdfReaderMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(SERVER_NAME, SERVER_VERSION)
                    .with_description(
                        "@sylphx/anymd sole-Rust MCP server (native binary; no TypeScript PDF runtime)",
                    )
                    .with_website_url("https://anymd.sylphx.com/"),
            )
            .with_instructions(SERVER_INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let supports_2026 = uses_2026_envelope(&context);
        let result = ListToolsResult {
            result_type: Some(ResultType::COMPLETE),
            tools: sanitized_tools(&self.tool_router),
            meta: supports_2026.then(|| server_result_meta(&self.get_info().server_info)),
            next_cursor: None,
            ttl_ms: supports_2026.then_some(0),
            cache_scope: supports_2026.then_some(CacheScope::Public),
        };
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let supports_2026 = uses_2026_envelope(&context);
        let mut response = match self.call_legacy(&request).await {
            Some(result) => CallToolResponse::Complete(result?),
            None => {
                let tool_context = ToolCallContext::new(self, request, context);
                self.tool_router.call(tool_context).await?
            }
        };
        if supports_2026 {
            if let CallToolResponse::Complete(ref mut result) = response {
                result.meta = Some(server_result_meta(&self.get_info().server_info));
            }
        }
        Ok(response)
    }

    /// Post-init `server/discover` (SEP-2575). Pre-init is handled by
    /// [`discover_compat::DiscoverAwareTransport`].
    async fn on_custom_request(
        &self,
        request: CustomRequest,
        _context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, ErrorData> {
        if request.method == discover_compat::SERVER_DISCOVER_METHOD {
            return Ok(CustomResult::new(discover_compat::discover_result_value(
                &self.get_info(),
            )));
        }
        Err(ErrorData::new(
            rmcp::model::ErrorCode::METHOD_NOT_FOUND,
            request.method,
            None,
        ))
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
mod sidecar_access_tests;
