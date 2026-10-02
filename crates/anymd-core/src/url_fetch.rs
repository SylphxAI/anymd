//! HTTP(S) PDF body fetch with DNS-pinned SSRF protection per redirect hop.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ureq::config::Config;
use ureq::tls::TlsConfig;
use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

use crate::ssrf::{is_private_ip, resolve_public_addrs_with, DnsResolver, SystemDnsResolver};

const MAX_REDIRECTS: usize = 5;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
struct PinnedResolver {
    expected_netloc: String,
    addresses: Vec<SocketAddr>,
}

impl Resolver for PinnedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        _config: &Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let netloc = netloc_of(uri);
        if netloc != self.expected_netloc {
            return Err(ureq::Error::Io(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing unvalidated network target '{netloc}' (expected '{}')",
                    self.expected_netloc
                ),
            )));
        }
        let mut resolved = self.empty();
        for address in &self.addresses {
            // The answer is a fixed-size array; keep the addresses that fit.
            if resolved.try_push(*address).is_err() {
                break;
            }
        }
        if resolved.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(resolved)
    }
}

/// `host:port` for a request URI, filling in the scheme's default port.
fn netloc_of(uri: &ureq::http::Uri) -> String {
    let port = uri.port_u16().or_else(|| match uri.scheme_str() {
        Some("http") => Some(80),
        Some("https") => Some(443),
        _ => None,
    });
    match (uri.host(), port) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        // Missing a host or a port cannot match any validated target.
        _ => String::new(),
    }
}

/// A fresh, pool-free agent per redirect hop: each hop is its own validation
/// and connection boundary. The URL keeps its hostname for Host and TLS SNI.
fn hop_agent(netloc: String, addresses: Vec<SocketAddr>, tls: &TlsConfig) -> ureq::Agent {
    hop_agent_timeout(netloc, addresses, tls, Duration::from_secs(TIMEOUT_SECS))
}
fn hop_agent_timeout(
    netloc: String,
    addresses: Vec<SocketAddr>,
    tls: &TlsConfig,
    timeout: Duration,
) -> ureq::Agent {
    let config = Config::builder()
        // The 2.x `.timeout(30s)` was a deadline for the whole call, body
        // included; `timeout_global` is 3.x's name for exactly that.
        .timeout_global(Some(timeout))
        // No redirects: this module follows them itself, one validated hop at a time.
        .max_redirects(0)
        // The 2.x agent built with `.try_proxy_from_env(false)`, and 3.x turns
        // the environment proxy on by default. Keep it off: the address this
        // agent connects to is the one `PinnedResolver` validated, and a proxy
        // would move the connection to a peer that no check here describes.
        .proxy(None)
        .max_idle_connections(0)
        .max_idle_connections_per_host(0)
        .tls_config(tls.clone())
        .build();
    ureq::Agent::with_parts(
        config,
        DefaultConnector::default(),
        PinnedResolver {
            expected_netloc: netloc,
            addresses,
        },
    )
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
    tls: &TlsConfig,
) -> Result<PathBuf, String>
where
    R: DnsResolver,
    F: Fn(IpAddr) -> bool + Copy,
{
    let fetched = fetch_url_with(url, resolver, is_denied, tls)?;
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

fn fetch_url_with<R, F>(
    url: &str,
    resolver: &R,
    is_denied: F,
    tls: &TlsConfig,
) -> Result<FetchedUrl, String>
where
    R: DnsResolver,
    F: Fn(IpAddr) -> bool + Copy,
{
    fetch_url_with_deadline(
        url,
        resolver,
        is_denied,
        tls,
        std::time::Instant::now() + Duration::from_secs(TIMEOUT_SECS),
    )
}
fn fetch_url_with_deadline<R, F>(
    url: &str,
    resolver: &R,
    is_denied: F,
    tls: &TlsConfig,
    deadline: std::time::Instant,
) -> Result<FetchedUrl, String>
where
    R: DnsResolver,
    F: Fn(IpAddr) -> bool + Copy,
{
    let mut current = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err("URL fetch deadline exceeded".into());
        }
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

        let agent = hop_agent_timeout(
            expected_netloc,
            addresses,
            tls,
            deadline.saturating_duration_since(std::time::Instant::now()),
        );

        let response = agent
            .get(parsed.as_str())
            .header(
                "User-Agent",
                concat!(
                    "anymd/",
                    env!("CARGO_PKG_VERSION"),
                    " (+https://github.com/SylphxAI/anymd)"
                ),
            )
            .header("Accept", "*/*")
            .call()
            .map_err(|e| format!("URL fetch failed: {e}"))?;

        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
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

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let final_url = parsed.to_string();
        let mut reader = response.into_body().into_reader().take(MAX_BYTES + 1);
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
    let tls = TlsConfig::default();
    if env_allow_private_ips() {
        fetch_url_to_temp_file_with(url, &SystemDnsResolver, |_| false, &tls)
    } else {
        fetch_url_to_temp_file_with(url, &SystemDnsResolver, is_private_ip, &tls)
    }
}

/// Fetch a URL body into memory with the same SSRF guard as
/// [`fetch_url_to_temp_file`] (every redirect hop is re-validated and pinned).
pub fn fetch_url(url: &str) -> Result<FetchedUrl, String> {
    let tls = TlsConfig::default();
    if env_allow_private_ips() {
        fetch_url_with(url, &SystemDnsResolver, |_| false, &tls)
    } else {
        fetch_url_with(url, &SystemDnsResolver, is_private_ip, &tls)
    }
}

/// Explicit caller deadline, retaining DNS pinning and redirect validation.
pub fn fetch_url_deadline(url: &str, deadline: std::time::Instant) -> Result<FetchedUrl, String> {
    let tls = TlsConfig::default();
    if env_allow_private_ips() {
        fetch_url_with_deadline(url, &SystemDnsResolver, |_| false, &tls, deadline)
    } else {
        fetch_url_with_deadline(url, &SystemDnsResolver, is_private_ip, &tls, deadline)
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

    fn default_tls() -> TlsConfig {
        TlsConfig::default()
    }

    /// The resolver is called without a deadline in these tests.
    fn no_deadline() -> NextTimeout {
        NextTimeout {
            after: ureq::unversioned::transport::time::Duration::NotHappening,
            reason: ureq::Timeout::Global,
        }
    }

    /// A self-signed certificate authority with a `localhost` leaf, as DER.
    fn test_ca_leaf_and_key() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let ca_key = rcgen::KeyPair::generate().expect("CA key");
        let mut ca_params = rcgen::CertificateParams::new(Vec::new()).expect("CA params");
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).expect("CA certificate");

        let leaf_key = rcgen::KeyPair::generate().expect("leaf key");
        let leaf_params =
            rcgen::CertificateParams::new(vec!["localhost".to_string()]).expect("leaf params");
        let issuer = rcgen::Issuer::from_params(&ca_params, &ca_key);
        let leaf = leaf_params
            .signed_by(&leaf_key, &issuer)
            .expect("leaf certificate");

        (
            ca.der().to_vec(),
            leaf.der().to_vec(),
            leaf_key.serialize_der(),
        )
    }

    /// Answer one request over TLS on a local port.
    fn spawn_one_https_response(
        leaf_der: Vec<u8>,
        leaf_key_der: Vec<u8>,
        response: &'static [u8],
    ) -> SocketAddr {
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(leaf_der)],
                rustls::pki_types::PrivateKeyDer::Pkcs8(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(leaf_key_der),
                ),
            )
            .expect("TLS server config");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind TLS server");
        let address = listener.local_addr().expect("TLS server address");
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept test request");
            let mut connection =
                rustls::ServerConnection::new(Arc::new(config)).expect("TLS server connection");
            let mut stream = rustls::Stream::new(&mut connection, &mut socket);
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read test request");
            stream.write_all(response).expect("write test response");
            let _ = stream.flush();
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
            &default_tls(),
        )
        .expect("fetch through pinned address");
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read(&path).unwrap(), b"%PDF-1.4");
        cleanup_temp_file(&path);
    }

    /// `max_redirects(0)` must hand the 3xx back as a response for this module
    /// to follow itself. ureq 3 only errors on the limit when it is above zero,
    /// so the hop still works; this pins that, and the follow-through, end to end.
    #[test]
    fn redirect_to_an_allowed_target_is_followed() {
        let destination = spawn_one_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n%PDF-1.4",
        );
        let redirect = spawn_one_response(Box::leak(
            format!(
                "HTTP/1.1 302 Found\r\nLocation: http://second.test:{}/final.pdf\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                destination.port()
            )
            .into_boxed_str(),
        )
        .as_bytes());
        let resolver = ScriptedResolver::new(HashMap::from([
            ("first.test".into(), vec![vec![redirect]]),
            ("second.test".into(), vec![vec![destination]]),
        ]));

        let path = fetch_url_to_temp_file_with(
            &format!("http://first.test:{}/redirect", redirect.port()),
            &resolver,
            |_| false,
            &default_tls(),
        )
        .expect("following the redirect must succeed");
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 2);
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
            &default_tls(),
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
        let error = Resolver::resolve(
            &resolver,
            &"https://other.test/".parse().unwrap(),
            &Config::default(),
            no_deadline(),
        )
        .expect_err("unexpected netloc must fail closed");
        assert!(
            matches!(&error, ureq::Error::Io(e) if e.kind() == io::ErrorKind::PermissionDenied),
            "{error}"
        );
    }

    #[test]
    fn pinned_resolver_answers_with_the_scheme_default_port() {
        let resolver = PinnedResolver {
            expected_netloc: "allowed.test:443".into(),
            addresses: vec!["8.8.8.8:443".parse().unwrap()],
        };
        let resolved = Resolver::resolve(
            &resolver,
            &"https://allowed.test/".parse().unwrap(),
            &Config::default(),
            no_deadline(),
        )
        .expect("default https port matches the pinned netloc");
        assert_eq!(
            &resolved[..],
            &["8.8.8.8:443".parse::<SocketAddr>().unwrap()]
        );
    }

    #[test]
    fn https_fetch_over_tls_against_a_local_server() {
        let (ca_der, leaf_der, leaf_key_der) = test_ca_leaf_and_key();
        let server = spawn_one_https_response(
            leaf_der,
            leaf_key_der,
            b"HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: 8\r\nConnection: close\r\n\r\n%PDF-1.4",
        );
        let resolver =
            ScriptedResolver::new(HashMap::from([("localhost".into(), vec![vec![server]])]));
        let tls = TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::new_with_certs(&[
                ureq::tls::Certificate::from_der(&ca_der).to_owned(),
            ]))
            .build();

        let fetched = fetch_url_with(
            &format!("https://localhost:{}/sample.pdf", server.port()),
            &resolver,
            |_| false,
            &tls,
        )
        .expect("https fetch through the pinned address");

        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fetched.bytes, b"%PDF-1.4");
        assert_eq!(fetched.content_type.as_deref(), Some("application/pdf"));
        assert_eq!(
            fetched.final_url,
            format!("https://localhost:{}/sample.pdf", server.port())
        );
    }
}
