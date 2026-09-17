mod command_provider;
pub mod discover_compat;
pub mod evidence;
pub mod http_transport;
mod ocr_evidence;
mod page_selection;
pub mod pdf_evidence;
pub mod read_pdf;
mod region_analysis_evidence;
pub mod schema;
pub mod search;
pub mod source_access;
pub mod tool_routes;
mod visual_evidence;

use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::tool::ToolCallContext,
    handler::server::wrapper::Parameters,
    model::{
        CacheScope, CallToolResponse, CustomRequest, CustomResult, Implementation, ListToolsResult,
        MetaObject, PaginatedRequestParams, ProtocolVersion, ResultType, ServerCapabilities,
        ServerInfo,
    },
    service::{RequestContext, RoleServer},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};

use crate::schema::{PdfEvidenceArgs, PdfEvidenceOperation, ReadPdfArgs, SearchPdfArgs};
use crate::source_access::SourceAccessPolicy;
use serde_json::Value;

pub const SERVER_NAME: &str = "citra";
/// Pure-Rust MCP server version — tracks the published npm product line when default.
pub const SERVER_VERSION: &str = "5.0.1";
pub const SERVER_INFO_META_KEY: &str = "io.modelcontextprotocol/serverInfo";
pub const SERVER_INSTRUCTIONS: &str =
    "@sylphx/citra sole-Rust MCP server (platform native binary). \
Capability-first semantic compatibility with TypeScript 3.0.14 interface contracts (ADR-0005/0006). \
No TypeScript PDF runtime is shipped in this package. Historical LKG: @sylphx/pdf-reader-mcp@3.0.14.";

fn omit_absent_optional_fields(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .filter_map(|(key, value)| {
                    (!value.is_null()).then(|| (key, omit_absent_optional_fields(value)))
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(omit_absent_optional_fields)
                .collect(),
        ),
        other => other,
    }
}

/// Formats defined by the JSON Schema specification (draft 2020-12). schemars
/// annotates Rust integer/float types with non-standard formats ("uint32",
/// "uint64", "double") that spec-strict client validators (AJV strict mode,
/// Zod, ...) reject or log as unknown on every tool call.
fn is_standard_schema_format(format: &str) -> bool {
    matches!(
        format,
        "date-time"
            | "date"
            | "time"
            | "duration"
            | "email"
            | "idn-email"
            | "hostname"
            | "idn-hostname"
            | "ipv4"
            | "ipv6"
            | "uri"
            | "uri-reference"
            | "iri"
            | "iri-reference"
            | "uri-template"
            | "json-pointer"
            | "relative-json-pointer"
            | "regex"
            | "uuid"
    )
}

fn sanitize_schema_formats(object: &mut serde_json::Map<String, Value>) {
    if matches!(object.get("format"), Some(Value::String(format)) if !is_standard_schema_format(format))
    {
        object.remove("format");
    }
    for value in object.values_mut() {
        match value {
            Value::Object(child) => sanitize_schema_formats(child),
            Value::Array(items) => {
                for item in items {
                    if let Value::Object(child) = item {
                        sanitize_schema_formats(child);
                    }
                }
            }
            _ => {}
        }
    }
}

fn sanitized_tools(router: &ToolRouter<PdfReaderMcp>) -> Vec<rmcp::model::Tool> {
    let mut tools = router.list_all();
    for tool in &mut tools {
        sanitize_schema_formats(std::sync::Arc::make_mut(&mut tool.input_schema));
        if let Some(output_schema) = &mut tool.output_schema {
            sanitize_schema_formats(std::sync::Arc::make_mut(output_schema));
        }
    }
    tools
}

#[derive(Clone)]
pub struct PdfReaderMcp {
    pub tool_router: ToolRouter<Self>,
    source_access: SourceAccessPolicy,
}

impl PdfReaderMcp {
    pub fn new() -> Self {
        Self::with_source_access(SourceAccessPolicy::unrestricted())
    }

    pub fn with_source_access(source_access: SourceAccessPolicy) -> Self {
        Self {
            tool_router: Self::tool_router(),
            source_access,
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
        description = "Primary PDF reader. Pure-Rust default: selectable text, markdown, chunks, tables, search, and evidence-oriented twin fields under capability-first semantic compatibility (ADR-0005)."
    )]
    pub async fn read_pdf(
        &self,
        Parameters(mut args): Parameters<ReadPdfArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        args.validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        self.source_access
            .admit_pdf_sources(&mut args.sources)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
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

    #[tool(
        description = "Searches extracted PDF text with page, snippet, geometry, and provenance. Optional OCR uses the bounded command provider."
    )]
    pub async fn search_pdf(
        &self,
        Parameters(mut args): Parameters<SearchPdfArgs>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        args.validate()
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        self.source_access
            .admit_pdf_sources(&mut args.sources)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
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

    #[tool(
        description = "Focused PDF evidence operations. Pure-Rust supports inspect, bounded page rendering/crops, opt-in bounded command-provider OCR, and region analysis via command, HTTP URL, or ollama/openai-compatible/lmstudio/llamacpp presets."
    )]
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

#[tool_handler]
impl ServerHandler for PdfReaderMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(SERVER_NAME, SERVER_VERSION)
                    .with_description(
                        "@sylphx/citra sole-Rust MCP server (native binary; no TypeScript PDF runtime)",
                    )
                    .with_website_url("https://sylphxai.github.io/pdf-reader-mcp/"),
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
        let tool_context = ToolCallContext::new(self, request, context);
        let mut response = self.tool_router.call(tool_context).await?;
        if supports_2026 {
            if let CallToolResponse::Complete(ref mut result) = response {
                result.meta = Some(server_result_meta(&self.get_info().server_info));
            }
        }
        Ok(response)
    }

    /// Post-init `server/discover` (SEP-2575). Pre-init is handled by
    /// [`discover_compat::DiscoverAwareTransport`].
    fn on_custom_request(
        &self,
        request: CustomRequest,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CustomResult, ErrorData>> + Send + '_ {
        async move {
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
}

#[cfg(test)]
mod tests {
    use super::{is_standard_schema_format, sanitize_schema_formats, sanitized_tools, PdfReaderMcp};
    use rmcp::handler::server::wrapper::Parameters;
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn rmcp_server_is_pure_rust() {
        let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        assert!(!src_dir.join("parity_bridge.rs").exists());
        let lib_rs = fs::read_to_string(src_dir.join("lib.rs")).expect("read lib.rs");
        let production_lib = lib_rs.split("#[cfg(test)]").next().unwrap_or(&lib_rs);
        assert!(production_lib.contains("read_pdf::read_pdf"));
        assert!(production_lib.contains("pdf_evidence::pdf_evidence"));
        assert!(production_lib.contains("search::search_pdf"));
        assert!(!production_lib.contains("parity_bridge"));
        let routes = fs::read_to_string(src_dir.join("tool_routes.rs")).expect("read tool_routes");
        assert!(routes.contains("RustCore"));
    }

    #[test]
    fn exposes_v3_tool_surface() {
        let tools = PdfReaderMcp::new().tool_router.list_all();
        let names: Vec<_> = tools.iter().map(|tool| tool.name.to_string()).collect();
        assert!(names.contains(&"read_pdf".to_string()));
        assert!(names.contains(&"search_pdf".to_string()));
        assert!(names.contains(&"pdf_evidence".to_string()));
    }

    #[test]
    fn tools_list_exposes_typed_object_schemas_not_empty_value() {
        let tools = PdfReaderMcp::new().tool_router.list_all();
        for tool in tools {
            let schema = tool.input_schema;
            let schema_value = serde_json::to_value(&schema).expect("schema json");
            // Must not be a bare free-form Value with no properties for our tools
            let ty = schema_value.get("type").and_then(|v| v.as_str());
            assert_eq!(
                ty,
                Some("object"),
                "tool {} schema type must be object, got {schema_value}",
                tool.name
            );
            let props = schema_value
                .get("properties")
                .and_then(|v| v.as_object())
                .expect("properties object");
            assert!(
                props.contains_key("sources"),
                "tool {} must document sources in inputSchema",
                tool.name
            );
            if tool.name == "search_pdf" {
                assert!(props.contains_key("query"));
            }
            if tool.name == "pdf_evidence" {
                assert!(props.contains_key("operation"));
            }
        }
    }

    #[test]
    fn tools_list_strips_non_standard_schema_formats() {
        // Raw router schemas carry schemars annotations (uint32, uint64,
        // double) that spec-strict client validators report as unknown
        // formats; the tools/list surface must not advertise them.
        let raw = serde_json::to_value(&PdfReaderMcp::new().tool_router.list_all()[0].input_schema)
            .expect("raw schema json");
        let raw_formats = schema_format_values(&raw);
        assert!(
            raw_formats.iter().any(|format| format == "uint32"),
            "precondition: raw schemars schema must contain a uint32 format, got {raw_formats:?}"
        );

        let tools = sanitized_tools(&PdfReaderMcp::new().tool_router);
        for tool in tools {
            let schema = serde_json::to_value(&tool.input_schema).expect("schema json");
            let formats = schema_format_values(&schema);
            for format in &formats {
                assert!(
                    is_standard_schema_format(format),
                    "tool {} advertises non-standard format {format}",
                    tool.name
                );
            }
            // Bounds are the contract; losing the format annotation must not
            // lose the numeric limits that back it.
            let raw_tool = serde_json::to_value(
                &PdfReaderMcp::new()
                    .tool_router
                    .list_all()
                    .into_iter()
                    .find(|raw_tool| raw_tool.name == tool.name)
                    .expect("raw tool")
                    .input_schema,
            )
            .expect("raw schema json");
            assert_eq!(
                schema_numeric_bounds(&schema),
                schema_numeric_bounds(&raw_tool),
                "tool {} numeric bounds changed during sanitization",
                tool.name
            );
        }
    }

    #[test]
    fn sanitize_schema_formats_keeps_standard_formats_and_bounds() {
        let mut object = serde_json::json!({
            "type": ["null", "integer"],
            "format": "uint32",
            "minimum": 1000,
            "maximum": 1000000,
            "standard": { "format": "uuid" },
            "nested": { "format": "double" },
            "list": [{ "format": "date-time" }, { "format": "uint64" }]
        })
        .as_object()
        .expect("object")
        .clone();
        sanitize_schema_formats(&mut object);
        let value = Value::Object(object);
        assert!(value.get("format").is_none());
        assert_eq!(value["minimum"], 1000);
        assert_eq!(value["maximum"], 1000000);
        assert_eq!(value["standard"]["format"], "uuid");
        assert!(value["nested"].get("format").is_none());
        assert_eq!(value["list"][0]["format"], "date-time");
        assert!(value["list"][1].get("format").is_none());
    }

    fn schema_format_values(value: &Value) -> Vec<String> {
        let mut formats = Vec::new();
        match value {
            Value::Object(object) => {
                if let Some(Value::String(format)) = object.get("format") {
                    formats.push(format.clone());
                }
                for child in object.values() {
                    formats.extend(schema_format_values(child));
                }
            }
            Value::Array(items) => {
                for item in items {
                    formats.extend(schema_format_values(item));
                }
            }
            _ => {}
        }
        formats
    }

    fn schema_numeric_bounds(value: &Value) -> Vec<(String, f64)> {
        let mut bounds = Vec::new();
        match value {
            Value::Object(object) => {
                for key in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"] {
                    if let Some(number) = object.get(key).and_then(|value| value.as_f64()) {
                        bounds.push((key.to_string(), number));
                    }
                }
                for child in object.values() {
                    bounds.extend(schema_numeric_bounds(child));
                }
            }
            Value::Array(items) => {
                for item in items {
                    bounds.extend(schema_numeric_bounds(item));
                }
            }
            _ => {}
        }
        bounds
    }

    fn tool_schema(name: &str) -> Value {
        let tool = PdfReaderMcp::new()
            .tool_router
            .list_all()
            .into_iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("missing tool {name}"));
        serde_json::to_value(tool.input_schema).expect("schema json")
    }

    fn v3_0_14_schema_oracle() -> Value {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test/fixtures/v3.0.14-input-schema-oracle.json"
        )))
        .expect("parse v3.0.14 schema oracle")
    }

    #[test]
    fn tools_list_schema_encodes_immutable_v3_0_14_enums_and_ranges() {
        let oracle = v3_0_14_schema_oracle();
        for fact in oracle["schemaFacts"].as_array().expect("schema facts") {
            let tool = fact["tool"].as_str().expect("fact tool");
            let pointer = fact["pointer"].as_str().expect("fact pointer");
            let schema = tool_schema(tool);
            let actual = schema.pointer(pointer).expect("schema fact pointer");
            let expected = &fact["expected"];
            let matches = match (actual.as_f64(), expected.as_f64()) {
                (Some(actual), Some(expected)) => actual == expected,
                _ => actual == expected,
            };
            assert!(
                matches,
                "v3.0.14 schema fact {tool}{pointer}: expected {expected}, got {actual}"
            );
        }

        for fact in oracle["enumFacts"].as_array().expect("enum facts") {
            let tool = fact["tool"].as_str().expect("enum tool");
            let schema_json = tool_schema(tool).to_string();
            for value in fact["values"].as_array().expect("enum values") {
                let value = value.as_str().expect("enum string");
                assert!(
                    schema_json.contains(&format!("\"{value}\"")),
                    "v3.0.14 {tool} missing enum {value}"
                );
            }
        }

        // v3.0.14 tools/list did not advertise prefer_speed (unknown keys were
        // stripped). Current product intentionally exposes prefer_speed as a
        // post-3.0.14 additive search_pdf property while dropInFor3014 stays false.
        const POST_3014_ADDITIVE: &[(&str, &str)] = &[("search_pdf", "prefer_speed")];
        for fact in oracle["absentProperties"]
            .as_array()
            .expect("absent properties")
        {
            let tool = fact["tool"].as_str().expect("absent tool");
            let property = fact["property"].as_str().expect("absent property");
            let schema = tool_schema(tool);
            let properties = schema["properties"].as_object().expect("tool properties");
            if POST_3014_ADDITIVE.contains(&(tool, property)) {
                assert!(
                    properties.contains_key(property),
                    "post-3.0.14 {tool} must expose additive property {property}"
                );
                let schema = &properties[property];
                let is_boolean = schema
                    .get("type")
                    .map(|value| match value {
                        Value::String(ty) => ty == "boolean",
                        Value::Array(items) => {
                            items.iter().any(|item| item.as_str() == Some("boolean"))
                        }
                        _ => false,
                    })
                    .unwrap_or(false)
                    || schema
                        .get("anyOf")
                        .or_else(|| schema.get("oneOf"))
                        .and_then(|value| value.as_array())
                        .map(|items| {
                            items.iter().any(|item| {
                                item.get("type").and_then(|ty| ty.as_str()) == Some("boolean")
                            })
                        })
                        .unwrap_or(false);
                assert!(
                    is_boolean,
                    "post-3.0.14 {tool}.{property} must be boolean-typed, got {schema}"
                );
            } else {
                assert!(
                    !properties.contains_key(property),
                    "v3.0.14 {tool} must not expose {property}"
                );
            }
        }

        // Provider-compat (#562): tools/list must not advertise exclusive path|url
        // via oneOf + not/required — Fireworks/OpenCode reject that keyword shape.
        // Runtime still enforces exactly-one-of via PdfSource::validate().
        let read_json = tool_schema("read_pdf").to_string();
        assert!(
            !read_json.contains("\"not\""),
            "read_pdf inputSchema must not use JSON Schema not for path|url XOR (OpenCode/Fireworks)"
        );
        assert!(
            !read_json.contains("\"oneOf\""),
            "read_pdf inputSchema must not use oneOf for path|url XOR after #562"
        );
    }

    #[tokio::test]
    async fn read_pdf_contains_malformed_cff_custom_encoding_panic() {
        // Regression for SylphxAI/pdf-reader-mcp#660: the CFF Custom encoding
        // panic must cross the MCP boundary as ErrorData, not kill the native
        // server process or leave the tool call timing out.
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../test/fixtures/differential/v3014-bug660-cff-custom-encoding-short-charset-v1.pdf",
        );
        let read = serde_json::from_value(serde_json::json!({
            "sources": [{"path": fixture}],
            "include_full_text": true,
        }))
        .expect("structurally valid read args");
        let error = PdfReaderMcp::new()
            .read_pdf(Parameters(read))
            .await
            .expect_err("read_pdf must return a structured MCP error");
        assert!(error.message.contains("malformed font encoding"));
    }

    #[tokio::test]
    async fn read_pdf_reports_malformed_inline_image_as_error() {
        // Regression for SylphxAI/pdf-reader-mcp#675: a malformed inline
        // image (missing `/CS` without `/IM`) panics inside lopdf 0.42's
        // content parser. It must cross the MCP boundary as ErrorData
        // naming the page, not kill the native server or time out the call.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("inline-malformed.pdf");
        let content = b"BI /W 1 /H 1 /BPC 1 ID \x00 EI\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << >> >>"
                .to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content.len(),
                String::from_utf8_lossy(content)
            ),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        std::fs::write(&path, pdf).expect("write PDF");
        let read = serde_json::from_value(serde_json::json!({
            "sources": [{"path": path}],
            "include_full_text": true,
        }))
        .expect("structurally valid read args");
        let error = PdfReaderMcp::new()
            .read_pdf(Parameters(read))
            .await
            .expect_err("read_pdf must return a structured MCP error");
        assert!(
            error.message.contains("invalid content stream (page 1)"),
            "unexpected message: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn read_pdf_survives_wellformed_inline_image() {
        // The #675 reproducer itself must read cleanly through the tool.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("inline-ok.pdf");
        let content = b"BI /W 1 /H 1 /IM true /BPC 1 ID \x00 EI\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << >> >>"
                .to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content.len(),
                String::from_utf8_lossy(content)
            ),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        std::fs::write(&path, pdf).expect("write PDF");
        let read = serde_json::from_value(serde_json::json!({
            "sources": [{"path": path}],
            "include_full_text": true,
        }))
        .expect("structurally valid read args");
        let result = PdfReaderMcp::new()
            .read_pdf(Parameters(read))
            .await
            .expect("read_pdf must complete on an inline-image PDF");
        assert!(!result.content.is_empty(), "expected a structured read_pdf result");
    }

    #[tokio::test]
    async fn search_pdf_handles_malformed_cid_cmap_fixture_without_aborting() {
        // Regression for SylphxAI/pdf-reader-mcp#608: a pdfTeX ToUnicode CMap
        // using 1-byte beginbfrange destinations (like <C5> <D6> <C5>) made the
        // upstream adobe-cmap-parser panic with "bad length of hexstring",
        // aborting the whole MCP server. The tool entrypoint must return a
        // structured result instead.
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential/v3014-bug608-cid-bfrange-odd-v1.pdf");
        let server = PdfReaderMcp::new();
        let search = serde_json::from_value(serde_json::json!({
            "sources": [{"path": fixture}],
            "query": "a"
        }))
        .expect("structurally valid search args");
        let result = server
            .search_pdf(Parameters(search))
            .await
            .expect("search_pdf must complete on a malformed CMap PDF");
        assert!(
            !result.content.is_empty(),
            "expected a structured search_pdf result"
        );

        let read = serde_json::from_value(serde_json::json!({
            "sources": [{"path": fixture}]
        }))
        .expect("structurally valid read args");
        let result = server
            .read_pdf(Parameters(read))
            .await
            .expect("read_pdf must complete on a malformed CMap PDF");
        assert!(
            !result.content.is_empty(),
            "expected a structured read_pdf result"
        );
    }

    #[tokio::test]
    async fn tool_entrypoints_reject_values_outside_v3_0_14_runtime_bounds() {
        let server = PdfReaderMcp::new();
        let read = serde_json::from_value(serde_json::json!({
            "sources": [{"path": "sample.pdf"}],
            "sample_pages": 21
        }))
        .expect("structurally valid read args");
        assert!(server.read_pdf(Parameters(read)).await.is_err());

        let search = serde_json::from_value(serde_json::json!({
            "sources": [{"path": "sample.pdf"}],
            "query": "needle",
            "context_chars": 1001
        }))
        .expect("structurally valid search args");
        assert!(server.search_pdf(Parameters(search)).await.is_err());

        let evidence = serde_json::from_value(serde_json::json!({
            "operation": "inspect",
            "sources": [{"path": "sample.pdf"}],
            "scale": 4.01
        }))
        .expect("structurally valid evidence args");
        assert!(server.pdf_evidence(Parameters(evidence)).await.is_err());
    }

    #[tokio::test]
    async fn all_tool_entrypoints_reject_paths_outside_the_native_allowlist() {
        let temp = tempfile::tempdir().expect("tempdir");
        let allowed = temp.path().join("allowed");
        std::fs::create_dir(&allowed).expect("allowed root");
        let outside = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/sample.pdf")
            .canonicalize()
            .expect("outside fixture");
        let policy = crate::source_access::SourceAccessPolicy::restricted_for_test(
            temp.path().to_path_buf(),
            &allowed,
        )
        .expect("restricted policy");
        let server = PdfReaderMcp::with_source_access(policy);

        let read = serde_json::from_value(serde_json::json!({
            "sources": [{"path": outside}]
        }))
        .expect("read args");
        let read_error = server
            .read_pdf(Parameters(read))
            .await
            .expect_err("read_pdf must reject outside path");
        assert!(read_error.message.contains("Access denied"));

        let search = serde_json::from_value(serde_json::json!({
            "sources": [{"path": outside}],
            "query": "needle"
        }))
        .expect("search args");
        let search_error = server
            .search_pdf(Parameters(search))
            .await
            .expect_err("search_pdf must reject outside path");
        assert!(search_error.message.contains("Access denied"));

        let evidence = serde_json::from_value(serde_json::json!({
            "operation": "inspect",
            "sources": [{"path": outside}]
        }))
        .expect("evidence args");
        let evidence_error = server
            .pdf_evidence(Parameters(evidence))
            .await
            .expect_err("pdf_evidence must reject outside path");
        assert!(evidence_error.message.contains("Access denied"));
    }

    #[test]
    fn rust_http_transport_module_is_wired_for_web_mcp() {
        let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let main_rs = fs::read_to_string(src_dir.join("main.rs")).expect("read main.rs");
        let http_rs =
            fs::read_to_string(src_dir.join("http_transport.rs")).expect("read http_transport.rs");
        assert!(main_rs.contains("http_transport::serve_http"));
        assert!(http_rs.contains("StreamableHttpService"));
        assert!(http_rs.contains("/mcp/health"));
    }

    #[test]
    fn server_version_tracks_package_or_experimental_marker() {
        // Sole-runtime default may advertise the package version; pre-cutover
        // builds keep an experimental marker.
        assert!(
            super::SERVER_VERSION.contains("experimental")
                || super::SERVER_VERSION.starts_with("0.")
                || super::SERVER_VERSION
                    .split('.')
                    .take(1)
                    .all(|part| part.chars().all(|c| c.is_ascii_digit()))
        );
        assert_ne!(super::SERVER_VERSION, "3.1.1");
    }
}
