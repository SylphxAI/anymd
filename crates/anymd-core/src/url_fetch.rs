//! HTTP(S) PDF body fetch with DNS-pinned SSRF protection per redirect hop.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ssrf::{is_private_ip, resolve_public_addrs_with, DnsResolver, SystemDnsResolver};

const MAX_REDIRECTS: usize = 5;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
struct PinnedResolver {
    expected_netloc: String,
    addresses: Vec<SocketAddr>,
}

impl ureq::Resolver for PinnedResolver {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<SocketAddr>> {
        if netloc != self.expected_netloc {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing unvalidated network target '{netloc}' (expected '{}')",
                    self.expected_netloc
                ),
            ));
        }
        Ok(self.addresses.clone())
    }
}

/// A fetched HTTP(S) body with the response facts a converter needs.
#[derive(Debug, Clone)]
pub struct FetchedUrl {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    /// URL after redirects (base for relative links).
    pub final_url: String,
}

fn fetch_url_to_temp_file_with<R, F>(
    url: &str,
    resolver: &R,
    is_denied: F,
) -> Result<PathBuf, String>
where
    R: DnsResolver,
    F: Fn(IpAddr) -> bool + Copy,
{
    let fetched = fetch_url_with(url, resolver, is_denied)?;
    let mut file = tempfile::Builder::new()
        .prefix("pdf-reader-mcp-")
        .suffix(".pdf")
        .tempfile()
        .map_err(|e| format!("secure temp file: {e}"))?;
    file.write_all(&fetched.bytes)
        .map_err(|e| format!("temp write: {e}"))?;
    let (_file, path) = file
        .keep()
        .map_err(|e| format!("persist secure temp file: {}", e.error))?;
    Ok(path)
}

fn fetch_url_with<R, F>(url: &str, resolver: &R, is_denied: F) -> Result<FetchedUrl, String>
where
    R: DnsResolver,
    F: Fn(IpAddr) -> bool + Copy,
{
    let mut current = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let parsed = url::Url::parse(&current).map_err(|e| format!("Invalid URL: {e}"))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err("Only http(s) URLs are allowed.".into());
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| "URL host is required.".to_string())?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| "URL port could not be determined.".to_string())?;
        let addresses = resolve_public_addrs_with(host, port, resolver, is_denied)?;
        let expected_netloc = format!("{host}:{port}");

        // A fresh, pool-free agent makes each hop a separate validation and
        // connection boundary. The URL retains the hostname for Host and TLS SNI.
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .redirects(0)
            .try_proxy_from_env(false)
            .max_idle_connections(0)
            .max_idle_connections_per_host(0)
            .resolver(PinnedResolver {
                expected_netloc,
                addresses,
            })
            .build();

        let response = agent
            .get(parsed.as_str())
            .set("User-Agent", concat!("anymd/", env!("CARGO_PKG_VERSION"), " (+https://github.com/SylphxAI/anymd)"))
            .set("Accept", "*/*")
            .call()
            .map_err(|e| format!("URL fetch failed: {e}"))?;

        let status = response.status();
        if (300..400).contains(&status) {
            let location = response
                .header("location")
                .ok_or_else(|| "Redirect without Location header.".to_string())?
                .to_string();
            current = parsed
                .join(&location)
                .map_err(|e| format!("Invalid redirect URL: {e}"))?
                .to_string();
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(format!("URL fetch returned HTTP {status}."));
        }

        let content_type = response.header("content-type").map(str::to_string);
        let final_url = parsed.to_string();
        let mut reader = response.into_reader().take(MAX_BYTES + 1);
        let mut bytes = Vec::new();
        std::io::copy(&mut reader, &mut bytes)
            .map_err(|e| format!("Failed to read URL body: {e}"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(format!(
                "URL body exceeds maximum size of {MAX_BYTES} bytes."
            ));
        }
        if bytes.is_empty() {
            return Err("URL body is empty.".into());
        }
        return Ok(FetchedUrl {
            bytes,
            content_type,
            final_url,
        });
    }
    Err(format!("Too many redirects (>{MAX_REDIRECTS})."))
}

fn env_allow_private_ips() -> bool {
    match std::env::var("MCP_PDF_ALLOW_PRIVATE_IPS") {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

/// Fetch a URL PDF body into a temporary file.
///
/// By default non-public addresses are rejected. Set
/// `MCP_PDF_ALLOW_PRIVATE_IPS=true` to match the TypeScript LKG opt-in that
/// permits loopback/private fetches for local fixtures and trusted networks.
pub fn fetch_url_to_temp_file(url: &str) -> Result<PathBuf, String> {
    if env_allow_private_ips() {
        fetch_url_to_temp_file_with(url, &SystemDnsResolver, |_| false)
    } else {
        fetch_url_to_temp_file_with(url, &SystemDnsResolver, is_private_ip)
    }
}

/// Fetch a URL body into memory with the same SSRF guard as
/// [`fetch_url_to_temp_file`] (every redirect hop is re-validated and pinned).
pub fn fetch_url(url: &str) -> Result<FetchedUrl, String> {
    if env_allow_private_ips() {
        fetch_url_with(url, &SystemDnsResolver, |_| false)
    } else {
        fetch_url_with(url, &SystemDnsResolver, is_private_ip)
    }
}

pub fn cleanup_temp_file(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, TcpListener};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[derive(Clone)]
    struct ScriptedResolver {
        answers: Arc<Mutex<HashMap<String, Vec<Vec<SocketAddr>>>>>,
        calls: Arc<AtomicUsize>,
    }

    impl ScriptedResolver {
        fn new(answers: HashMap<String, Vec<Vec<SocketAddr>>>) -> Self {
            Self {
                answers: Arc::new(Mutex::new(answers)),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl DnsResolver for ScriptedResolver {
        fn resolve(&self, host: &str, _port: u16) -> io::Result<Vec<SocketAddr>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut answers = self.answers.lock().expect("resolver answers lock");
            let script = answers.get_mut(host).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, format!("no DNS script for {host}"))
            })?;
            if script.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("DNS script exhausted for {host}"),
                ));
            }
            Ok(script.remove(0))
        }
    }

    fn spawn_one_response(response: &'static [u8]) -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read test request");
            stream.write_all(response).expect("write test response");
        });
        address
    }

    #[test]
    fn one_dns_resolution_is_pinned_to_the_actual_connection() {
        let server = spawn_one_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n%PDF-1.4",
        );
        let rebound = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), server.port());
        let resolver = ScriptedResolver::new(HashMap::from([(
            "rebind.test".into(),
            vec![vec![server], vec![rebound]],
        )]));

        let path = fetch_url_to_temp_file_with(
            &format!("http://rebind.test:{}/sample.pdf", server.port()),
            &resolver,
            |_| false,
        )
        .expect("fetch through pinned address");
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read(&path).unwrap(), b"%PDF-1.4");
        cleanup_temp_file(&path);
    }

    #[test]
    fn redirect_target_is_revalidated_before_connection() {
        let redirect = spawn_one_response(
            b"HTTP/1.1 302 Found\r\nLocation: http://blocked.test:6553/private.pdf\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let blocked: SocketAddr = "127.0.0.2:6553".parse().unwrap();
        let resolver = ScriptedResolver::new(HashMap::from([
            ("first.test".into(), vec![vec![redirect]]),
            ("blocked.test".into(), vec![vec![blocked]]),
        ]));

        let error = fetch_url_to_temp_file_with(
            &format!("http://first.test:{}/redirect", redirect.port()),
            &resolver,
            |ip| ip == blocked.ip(),
        )
        .expect_err("redirect to denied target must fail");
        assert!(error.contains("non-public address"), "{error}");
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn pinned_resolver_rejects_unexpected_netloc() {
        let resolver = PinnedResolver {
            expected_netloc: "allowed.test:443".into(),
            addresses: vec!["8.8.8.8:443".parse().unwrap()],
        };
        let error = ureq::Resolver::resolve(&resolver, "other.test:443")
            .expect_err("unexpected netloc must fail closed");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}
