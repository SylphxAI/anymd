//! Streamable HTTP Web MCP transport for anymd (rmcp).
//!
//! Mirrors the TS adapter surface: `/mcp`, `/mcp/health`, optional `X-API-Key`, CORS.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, options},
    Json, Router,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, tower::StreamableHttpService, StreamableHttpServerConfig,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{source_access::SourceAccessPolicy, PdfReaderMcp};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8080;

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub host: String,
    pub port: u16,
    pub api_key: Option<String>,
    pub cors_origin: Option<String>,
    pub allow_unauthenticated_remote: bool,
}

impl HttpConfig {
    pub fn from_env() -> Self {
        let host = std::env::var("MCP_HTTP_HOST")
            .or_else(|_| std::env::var("PDF_READER_MCP_HTTP_HOST"))
            .unwrap_or_else(|_| DEFAULT_HOST.to_string());

        let port = std::env::var("MCP_HTTP_PORT")
            .or_else(|_| std::env::var("PDF_READER_MCP_HTTP_PORT"))
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_PORT);

        let api_key = std::env::var("MCP_API_KEY")
            .or_else(|_| std::env::var("PDF_READER_MCP_API_KEY"))
            .ok()
            .filter(|value| !value.is_empty());

        let cors_origin = std::env::var("MCP_CORS_ORIGIN")
            .or_else(|_| std::env::var("PDF_READER_MCP_CORS_ORIGIN"))
            .ok()
            .filter(|value| !value.is_empty());

        let allow_unauthenticated_remote = std::env::var("MCP_ALLOW_UNAUTHENTICATED_REMOTE")
            .ok()
            .is_some_and(|value| {
                matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true")
            });

        Self {
            host,
            port,
            api_key,
            cors_origin,
            allow_unauthenticated_remote,
        }
    }

    pub fn socket_addr(&self) -> anyhow::Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .map_err(|error| anyhow::anyhow!("invalid MCP HTTP bind address {addr}: {error}"))
    }

    pub fn is_loopback_host(&self) -> bool {
        self.host == "localhost"
            || self.host == "::1"
            || self.host == "127.0.0.1"
            || self.host.starts_with("127.")
    }

    pub fn validate_exposure(&self) -> anyhow::Result<()> {
        if !self.is_loopback_host() && self.api_key.is_none() && !self.allow_unauthenticated_remote
        {
            anyhow::bail!(
                "Refusing unauthenticated non-loopback MCP bind on {}. Set MCP_API_KEY or explicitly set MCP_ALLOW_UNAUTHENTICATED_REMOTE=true.",
                self.host
            );
        }
        Ok(())
    }

    /// Host authorities accepted by rmcp Streamable HTTP DNS-rebinding protection.
    ///
    /// Defaults: loopbacks + the configured bind host (and host:port). Override with
    /// `MCP_ALLOWED_HOSTS` / `PDF_READER_MCP_ALLOWED_HOSTS` (comma-separated).
    pub fn allowed_hosts(&self) -> Vec<String> {
        if let Ok(raw) = std::env::var("MCP_ALLOWED_HOSTS")
            .or_else(|_| std::env::var("PDF_READER_MCP_ALLOWED_HOSTS"))
        {
            let hosts: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect();
            if !hosts.is_empty() {
                return hosts;
            }
        }

        let mut hosts = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
        ];
        if !hosts.iter().any(|host| host == &self.host) {
            hosts.push(self.host.clone());
        }
        let with_port = format!("{}:{}", self.host, self.port);
        if !hosts.iter().any(|host| host == &with_port) {
            hosts.push(with_port);
        }
        if self.is_loopback_host() {
            for extra in [
                "0.0.0.0".to_string(),
                format!("0.0.0.0:{}", self.port),
                format!("localhost:{}", self.port),
                format!("127.0.0.1:{}", self.port),
            ] {
                if !hosts.iter().any(|host| host == &extra) {
                    hosts.push(extra);
                }
            }
        }
        hosts
    }
}

pub fn transport_from_env() -> Option<&'static str> {
    let transport = std::env::var("MCP_TRANSPORT")
        .or_else(|_| std::env::var("PDF_READER_MCP_TRANSPORT"))
        .ok()?;
    if transport == "http" {
        Some("http")
    } else {
        None
    }
}

pub fn is_api_key_valid(configured_key: &str, presented: Option<&str>) -> bool {
    let Some(provided) = presented.filter(|value| !value.is_empty()) else {
        return false;
    };
    let expected = Sha256::digest(configured_key.as_bytes());
    let actual = Sha256::digest(provided.as_bytes());
    expected.ct_eq(&actual).into()
}

fn apply_cors(headers: &mut HeaderMap, origin: Option<&str>) {
    let Some(origin) = origin else {
        return;
    };
    if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert("access-control-allow-origin", value);
    }
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static(
            "Content-Type, Accept, MCP-Protocol-Version, Mcp-Session-Id, X-API-Key",
        ),
    );
}

async fn cors_middleware(
    State(config): State<Arc<HttpConfig>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    if method == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        apply_cors(response.headers_mut(), config.cors_origin.as_deref());
        return response;
    }

    let mut response = next.run(request).await;
    apply_cors(response.headers_mut(), config.cors_origin.as_deref());
    response
}

async fn api_key_middleware(
    State(config): State<Arc<HttpConfig>>,
    headers: HeaderMap,
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let Some(expected_key) = config.api_key.as_deref() else {
        return Ok(next.run(request).await);
    };

    let path = request.uri().path();
    if request.method() == Method::GET && path.ends_with("/health") {
        return Ok(next.run(request).await);
    }

    let presented = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());

    if is_api_key_valid(expected_key, presented) {
        return Ok(next.run(request).await);
    }

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32001,
            "message": "Unauthorized: missing or invalid X-API-Key header"
        }
    });
    let mut response = (StatusCode::UNAUTHORIZED, Json(body)).into_response();
    response
        .headers_mut()
        .insert("www-authenticate", HeaderValue::from_static("X-API-Key"));
    Err(response)
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

pub async fn serve_http(
    config: HttpConfig,
    source_access: SourceAccessPolicy,
) -> anyhow::Result<()> {
    config.validate_exposure()?;
    let addr = config.socket_addr()?;
    let shared_config = Arc::new(config);
    let cancellation = tokio_util::sync::CancellationToken::new();

    let mcp_service = StreamableHttpService::new(
        move || Ok(PdfReaderMcp::with_source_access(source_access.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_cancellation_token(cancellation.child_token())
            .with_allowed_hosts(shared_config.allowed_hosts()),
    );

    let mcp_router = Router::new()
        .route("/health", get(health_check))
        .route("/health", options(|| async { StatusCode::NO_CONTENT }))
        .fallback_service(mcp_service)
        .layer(middleware::from_fn_with_state(
            shared_config.clone(),
            api_key_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            shared_config.clone(),
            cors_middleware,
        ));

    let app = Router::new().nest("/mcp", mcp_router);

    eprintln!("[anymd] Streamable HTTP MCP listening on http://{addr}/mcp");
    eprintln!("[anymd] Health check: http://{addr}/mcp/health");
    if let Some(api_key) = shared_config.api_key.as_deref() {
        let _ = api_key;
        eprintln!("[anymd] API key authentication enabled (X-API-Key header)");
    } else if shared_config.allow_unauthenticated_remote {
        eprintln!(
            "[anymd] WARNING: explicit unauthenticated remote bind enabled for {}.",
            shared_config.host
        );
    }
    if let Some(origin) = shared_config.cors_origin.as_deref() {
        eprintln!("[anymd] CORS allowed origin: {origin}");
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let cancel = cancellation.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.ok();
            cancel.cancel();
        })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_validation_matches_ts_sha256_digest_policy() {
        assert!(is_api_key_valid("secret-key", Some("secret-key")));
        assert!(!is_api_key_valid("secret-key", Some("wrong-key")));
        assert!(!is_api_key_valid("secret-key", None));
        assert!(!is_api_key_valid("secret-key", Some("")));
    }

    #[test]
    fn http_config_defaults_to_loopback_port_8080() {
        let config = HttpConfig {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            api_key: None,
            cors_origin: None,
            allow_unauthenticated_remote: false,
        };
        assert!(config.is_loopback_host());
        assert_eq!(config.socket_addr().expect("addr").port(), 8080);
    }

    #[test]
    fn allowed_hosts_include_bind_host_and_loopbacks() {
        let config = HttpConfig {
            host: "127.0.0.1".to_string(),
            port: 8080,
            api_key: None,
            cors_origin: None,
            allow_unauthenticated_remote: false,
        };
        let hosts = config.allowed_hosts();
        assert!(hosts.iter().any(|host| host == "127.0.0.1"));
        assert!(hosts.iter().any(|host| host == "localhost"));
        assert!(hosts.iter().any(|host| host == "127.0.0.1:8080"));
    }

    #[test]
    fn non_loopback_bind_requires_authentication_or_explicit_break_glass() {
        let mut config = HttpConfig {
            host: "0.0.0.0".to_string(),
            port: 8080,
            api_key: None,
            cors_origin: None,
            allow_unauthenticated_remote: false,
        };
        assert!(config.validate_exposure().is_err());

        config.api_key = Some("secret".into());
        assert!(config.validate_exposure().is_ok());

        config.api_key = None;
        config.allow_unauthenticated_remote = true;
        assert!(config.validate_exposure().is_ok());
    }
}
