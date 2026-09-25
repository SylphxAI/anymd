//! MCP `server/discover` (SEP-2575) compatibility for dual-era clients.
//!
//! Modern MCP clients (including Gemini Antigravity CLI) probe with
//! `server/discover` *before* the legacy `initialize` handshake. rmcp 1.8.0 only
//! tolerates `ping` in that pre-init window and otherwise aborts the transport
//! (`expect initialized request`), which surfaces as EOF to the client.
//!
//! This module:
//! 1. builds a draft-shaped DiscoverResult from our ServerInfo
//! 2. wraps any RoleServer transport so pre-init `server/discover` is answered
//!    without killing the connection, then `initialize` can proceed

use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;

use rmcp::model::{
    ClientJsonRpcMessage, ClientRequest, CustomRequest, CustomResult, ProtocolVersion, ServerInfo,
    ServerJsonRpcMessage, ServerResult,
};
use rmcp::service::RoleServer;
use rmcp::transport::Transport;
use serde_json::{json, Value};

/// Canonical MCP method name for SEP-2575 discovery.
pub const SERVER_DISCOVER_METHOD: &str = "server/discover";

const SERVER_INFO_META_KEY: &str = "io.modelcontextprotocol/serverInfo";

/// Build a draft-shaped `server/discover` result from the server's initialize identity.
///
/// Shape follows
/// <https://modelcontextprotocol.io/specification/draft/server/discover>:
/// `resultType`, `supportedVersions`, `capabilities`, `_meta.serverInfo`,
/// `instructions`.
pub fn discover_result_value(info: &ServerInfo) -> Value {
    let supported_versions: Vec<String> = ProtocolVersion::KNOWN_VERSIONS
        .iter()
        .map(|version| version.as_str().to_string())
        .collect();

    let mut server_info = json!({
        "name": info.server_info.name,
        "version": info.server_info.version,
    });
    if let Some(title) = &info.server_info.title {
        server_info["title"] = json!(title);
    }
    if let Some(description) = &info.server_info.description {
        server_info["description"] = json!(description);
    }
    if let Some(website_url) = &info.server_info.website_url {
        server_info["websiteUrl"] = json!(website_url);
    }

    let mut result = json!({
        "resultType": "complete",
        "supportedVersions": supported_versions,
        "capabilities": info.capabilities,
        "cacheScope": "private",
        "ttlMs": 3_600_000,
        "_meta": {
            SERVER_INFO_META_KEY: server_info,
        },
    });

    if let Some(instructions) = &info.instructions {
        result["instructions"] = json!(instructions);
    }

    result
}

/// True when the client request is SEP-2575 `server/discover`.
pub fn is_server_discover_request(request: &ClientRequest) -> bool {
    match request {
        ClientRequest::DiscoverRequest(_) => true,
        ClientRequest::CustomRequest(CustomRequest { method, .. }) => {
            method == SERVER_DISCOVER_METHOD
        }
        _ => false,
    }
}

/// Transport wrapper that answers pre-init `server/discover` and keeps the
/// connection open for the subsequent legacy `initialize` handshake.
pub struct DiscoverAwareTransport<T> {
    inner: T,
    discover_result: Arc<Value>,
}

impl<T> DiscoverAwareTransport<T> {
    pub fn new(inner: T, discover_result: Value) -> Self {
        Self {
            inner,
            discover_result: Arc::new(discover_result),
        }
    }
}

impl<T> Transport<RoleServer> for DiscoverAwareTransport<T>
where
    T: Transport<RoleServer> + 'static,
{
    type Error = T::Error;

    fn name() -> Cow<'static, str> {
        Cow::Borrowed("discover-aware-transport")
    }

    fn send(
        &mut self,
        item: ServerJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    fn receive(&mut self) -> impl Future<Output = Option<ClientJsonRpcMessage>> + Send {
        async {
            loop {
                let message = self.inner.receive().await?;
                let ClientJsonRpcMessage::Request(request) = &message else {
                    return Some(message);
                };

                if !is_server_discover_request(&request.request) {
                    return Some(message);
                }

                let response = ServerJsonRpcMessage::response(
                    ServerResult::CustomResult(CustomResult::new(
                        self.discover_result.as_ref().clone(),
                    )),
                    request.id.clone(),
                );

                if self.inner.send(response).await.is_err() {
                    return None;
                }
                // Answer discover and wait for the next client message (usually initialize).
            }
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{Implementation, ServerCapabilities};

    fn sample_info() -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("anymd", "5.0.0")
                    .with_description("test description")
                    .with_website_url("https://example.test/"),
            )
            .with_instructions("test instructions")
    }

    #[test]
    fn discover_result_includes_required_fields() {
        let value = discover_result_value(&sample_info());
        assert_eq!(value["resultType"], "complete");
        assert!(value["supportedVersions"]
            .as_array()
            .expect("supportedVersions array")
            .iter()
            .any(|v| v.as_str() == Some("2026-07-28")));
        assert!(value["capabilities"]["tools"].is_object());
        assert_eq!(value["cacheScope"], "private");
        assert_eq!(value["ttlMs"], 3_600_000);
        assert_eq!(value["_meta"][SERVER_INFO_META_KEY]["name"], "anymd");
        assert_eq!(value["_meta"][SERVER_INFO_META_KEY]["version"], "5.0.0");
        assert_eq!(value["instructions"], "test instructions");
    }

    #[test]
    fn is_server_discover_matches_method_only() {
        let discover = ClientRequest::CustomRequest(CustomRequest::new(
            SERVER_DISCOVER_METHOD,
            Some(json!({})),
        ));
        let other = ClientRequest::CustomRequest(CustomRequest::new("tools/list", Some(json!({}))));
        assert!(is_server_discover_request(&discover));
        assert!(!is_server_discover_request(&other));
    }
}
