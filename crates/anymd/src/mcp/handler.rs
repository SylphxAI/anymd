use rmcp::{
    handler::server::tool::ToolCallContext,
    model::{
        CacheScope, CallToolResponse, CustomRequest, CustomResult, Implementation, ListToolsResult,
        MetaObject, PaginatedRequestParams, ProtocolVersion, ResultType, ServerCapabilities,
        ServerConfig,
    },
    service::{RequestContext, RoleServer},
    tool_handler, ErrorData, ServerHandler,
};

use super::PdfReaderMcp;
use crate::discover_compat;
use crate::schema_sanitize::sanitized_tools;
use crate::{SERVER_INFO_META_KEY, SERVER_INSTRUCTIONS, SERVER_NAME, SERVER_VERSION};

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
