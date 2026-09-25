//! Regression for #598: Gemini Antigravity (and other dual-era clients) send
//! `server/discover` before `initialize`. The process must answer discover and
//! still complete the legacy initialize handshake.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

fn resolve_mcp_binary() -> PathBuf {
    for relative in [
        "bin/native/anymd",
        "target/release/anymd",
        "target/debug/anymd",
    ] {
        let candidate = repo_root().join(relative);
        if candidate.is_file() {
            return candidate;
        }
    }
    // Fall back to cargo-built test binary next to this test via CARGO_BIN_EXE
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_anymd") {
        return PathBuf::from(path);
    }
    panic!("anymd binary not found; run cargo build -p pdf-reader-mcp-server");
}

struct StdioClient {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl StdioClient {
    fn spawn() -> Self {
        let binary = resolve_mcp_binary();
        let mut child = Command::new(&binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("MCP_TRANSPORT")
            .env_remove("PDF_READER_MCP_TRANSPORT")
            .spawn()
            .unwrap_or_else(|error| panic!("spawn {}: {error}", binary.display()));
        let stdout = child.stdout.take().expect("stdout");
        let stdin = child.stdin.take().expect("stdin");
        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        }
    }

    fn write_message(&mut self, message: &Value) {
        let payload = serde_json::to_string(message).expect("serialize");
        writeln!(self.stdin, "{payload}").expect("write stdin");
        self.stdin.flush().expect("flush stdin");
    }

    fn read_response(&mut self, id: u64) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut line = String::new();
        loop {
            if std::time::Instant::now() > deadline {
                let _ = self.child.kill();
                panic!("timed out waiting for id={id}");
            }
            line.clear();
            match self.stdout.read_line(&mut line) {
                Ok(0) => panic!("server closed stdout while waiting for id={id}"),
                Ok(_) => {}
                Err(error) => panic!("read stdout: {error}"),
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let payload: Value = serde_json::from_str(trimmed)
                .unwrap_or_else(|error| panic!("parse `{trimmed}`: {error}"));
            if payload.get("id").and_then(Value::as_u64) == Some(id) {
                return payload;
            }
        }
    }

    fn send_request(&mut self, method: &str, params: Value) -> Value {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        self.read_response(id)
    }

    fn send_notification(&mut self, method: &str, params: Value) {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn server_discover_before_initialize_keeps_session_alive() {
    let mut client = StdioClient::spawn();

    // Gemini Antigravity dual-era probe shape.
    let discover = client.send_request(
        "server/discover",
        json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientInfo": {
                    "name": "antigravity-client",
                    "version": "v1.0.0"
                },
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }),
    );

    assert!(
        discover.get("error").is_none(),
        "discover must not fail: {discover}"
    );
    let result = discover
        .get("result")
        .expect("discover result")
        .as_object()
        .expect("result object");
    assert_eq!(result.get("resultType").and_then(Value::as_str), Some("complete"));
    assert!(
        result
            .get("supportedVersions")
            .and_then(Value::as_array)
            .is_some_and(|versions| !versions.is_empty()),
        "supportedVersions required: {discover}"
    );
    assert_eq!(
        discover
            // JSON Pointer escapes '/' in key names as ~1
            .pointer("/result/_meta/io.modelcontextprotocol~1serverInfo/name")
            .and_then(Value::as_str),
        Some("anymd")
    );

    // Legacy initialize must still succeed on the same connection.
    let initialize = client.send_request(
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "discover-preinit-test", "version": "1.0.0" },
        }),
    );
    assert!(
        initialize.get("error").is_none(),
        "initialize after discover must succeed: {initialize}"
    );
    assert_eq!(
        initialize
            .pointer("/result/serverInfo/name")
            .and_then(Value::as_str),
        Some("anymd")
    );

    client.send_notification("notifications/initialized", json!({}));

    let tools = client.send_request("tools/list", json!({}));
    assert!(
        tools.get("error").is_none(),
        "tools/list after discover+initialize must succeed: {tools}"
    );
    let tool_names: Vec<&str> = tools
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();
    assert!(
        tool_names.iter().any(|name| *name == "read"),
        "expected read in {tool_names:?}"
    );
}

#[test]
fn repeated_server_discover_before_initialize_is_tolerated() {
    let mut client = StdioClient::spawn();

    for _ in 0..2 {
        let discover = client.send_request("server/discover", json!({}));
        assert!(
            discover.get("error").is_none(),
            "discover must succeed: {discover}"
        );
    }

    let initialize = client.send_request(
        "initialize",
        json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "discover-repeat-test", "version": "1.0.0" },
        }),
    );
    assert!(
        initialize.get("error").is_none(),
        "initialize after repeated discover must succeed: {initialize}"
    );
}
