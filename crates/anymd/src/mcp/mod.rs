mod handler;
mod inspect;
mod legacy;

pub use legacy::LEGACY_TOOL_NAMES;

use rmcp::{
    handler::server::router::tool::ToolRouter, handler::server::wrapper::Parameters, tool,
    tool_router, ErrorData,
};

use crate::schema::{InspectArgs, OutlineArgs, ReadArgs, SearchArgs};
use crate::source_access::SourceAccessPolicy;
use crate::{lean, outline, pro};

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
