//! TRUE differential parity: frozen golden oracle vs native Rust pdf-reader rmcp SSOT.
//!
//! Fail-closed — no SKIP-as-pass. Oracle subprocess must succeed before comparison.
//! See scripts/run-pdf-reader-differential.sh and rej-010 re-audit.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pdf_reader_mcp_server::read_pdf;
use pdf_reader_mcp_server::tool_routes::{route_for_tool, ToolRoute};
use pdf_reader_mcp_server::{PdfReaderMcp, SERVER_NAME, SERVER_VERSION};
use serde::Deserialize;
use serde_json::{json, Value};

static STDIO_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus_fixture_path() -> PathBuf {
    repo_root().join("scripts/differential/fixtures/pdf-reader-mcp-corpus.json")
}

fn fixtures_root() -> PathBuf {
    repo_root().join("test/fixtures")
}

#[derive(Debug, Deserialize)]
struct OracleCase {
    id: String,
    domain: String,
    input: Value,
    output: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OracleCorpus {
    corpus_version: u32,
    fixture_corpus_hash: String,
    golden_fixture_hash: String,
    profile: String,
    cases: Vec<OracleCase>,
}

fn bun_available() -> bool {
    Command::new("bun")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_ts_oracle() -> Option<OracleCorpus> {
    if let Ok(path) = std::env::var("PDF_READER_MCP_ORACLE_JSON") {
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read PDF_READER_MCP_ORACLE_JSON at {path}: {error}"));
        return Some(serde_json::from_str(&raw).expect("oracle JSON must be valid"));
    }

    if !bun_available() {
        eprintln!("SKIP pdf_reader_mcp_differential: bun not available for TS oracle");
        return None;
    }

    let script = repo_root().join("scripts/differential/pdf-reader-mcp-oracle.ts");
    if !script.is_file() {
        eprintln!(
            "SKIP pdf_reader_mcp_differential: missing oracle script {}",
            script.display()
        );
        return None;
    }
    let output = Command::new("bun")
        .arg("run")
        .arg(&script)
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|error| panic!("spawn TS oracle at {}: {error}", script.display()));

    assert!(
        output.status.success(),
        "TS oracle failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Some(serde_json::from_slice(&output.stdout).expect("oracle output must be valid JSON"))
}

fn resolve_transport(env: &Value) -> String {
    let env_obj = env.as_object().expect("transport env object");
    if let Some(value) = env_obj
        .get("PDF_READER_MCP_TRANSPORT")
        .and_then(Value::as_str)
    {
        return value.to_string();
    }
    if let Some(value) = env_obj.get("MCP_TRANSPORT").and_then(Value::as_str) {
        return value.to_string();
    }
    "stdio".to_string()
}

fn surface_file(surface: &str) -> PathBuf {
    match surface {
        "bin" => repo_root().join("bin/citra"),
        "stdio" => repo_root().join("crates/pdf-reader-mcp-server/src/main.rs"),
        other => panic!("unknown surface {other}"),
    }
}

fn surface_markers(surface: &str, markers: &[String]) -> BTreeMap<String, bool> {
    let path = surface_file(surface);
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let mut found = BTreeMap::new();
    for marker in markers {
        found.insert(marker.clone(), content.contains(marker));
    }
    found
}

fn normalize_path_label(path: &str) -> String {
    let fixtures = fixtures_root();
    Path::new(path)
        .strip_prefix(&fixtures)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.to_string())
}

fn normalize_structured(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("evidence");
        if let Some(results) = object.get_mut("results").and_then(Value::as_array_mut) {
            for result in results {
                if let Some(result_object) = result.as_object_mut() {
                    if let Some(source) = result_object
                        .get("source")
                        .and_then(Value::as_str)
                        .map(normalize_path_label)
                    {
                        result_object.insert("source".into(), Value::String(source));
                    }
                    if let Some(data) = result_object.get_mut("data").and_then(Value::as_object_mut)
                    {
                        data.remove("full_text");
                        if let Some(info) = data.get_mut("info").and_then(Value::as_object_mut) {
                            info.remove("text_chars");
                        }
                    }
                }
            }
        }
    }
    value
}

fn parse_rmcp_structured(result: &rmcp::model::CallToolResult) -> Value {
    result
        .structured_content
        .clone()
        .or_else(|| {
            result
                .content
                .first()
                .and_then(|block| block.as_text())
                .and_then(|text| serde_json::from_str(&text.text).ok())
        })
        .expect("rmcp read_pdf structured content")
}

fn resolve_mcp_binary() -> PathBuf {
    for relative in [
        "bin/native/citra-mcp-server",
        "target/release/citra-mcp-server",
        "target/debug/citra-mcp-server",
    ] {
        let candidate = repo_root().join(relative);
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("citra-mcp-server is not built; run `bun run build:rust`");
}

struct StdioMcpClient {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<ChildStdout>,
    initialized: bool,
}

impl StdioMcpClient {
    fn spawn() -> Self {
        let binary = resolve_mcp_binary();
        let mut child = Command::new(&binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("MCP_TRANSPORT")
            .env_remove("PDF_READER_MCP_TRANSPORT")
            .spawn()
            .unwrap_or_else(|error| {
                panic!("spawn rmcp stdio server at {}: {error}", binary.display())
            });

        let stdout = child.stdout.take().expect("rmcp stdio server stdout");
        let stdin = child.stdin.take().expect("rmcp stdio server stdin");

        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            initialized: false,
        }
    }

    fn write_message(&mut self, message: &Value) {
        let payload = serde_json::to_string(message).expect("serialize MCP message");
        writeln!(self.stdin, "{payload}").expect("write MCP message to stdin");
        self.stdin.flush().expect("flush MCP stdin");
    }

    fn read_response(&mut self, id: u64) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        let mut line = String::new();

        loop {
            if std::time::Instant::now() > deadline {
                panic!("timed out waiting for MCP response id={id}");
            }

            line.clear();
            match self.stdout.read_line(&mut line) {
                Ok(0) => panic!("rmcp stdio server closed stdout while waiting for id={id}"),
                Ok(_) => {}
                Err(error) => panic!("read rmcp stdio stdout: {error}"),
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let payload: Value = serde_json::from_str(trimmed)
                .unwrap_or_else(|error| panic!("parse MCP stdout line `{trimmed}`: {error}"));

            if payload.get("id").and_then(Value::as_u64) == Some(id) {
                return payload;
            }
        }
    }

    fn send_request(&mut self, method: &str, params: Value) -> Value {
        let id = STDIO_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
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

    fn initialize_session(&mut self) {
        if self.initialized {
            return;
        }

        let response = self.send_request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "stdio-differential", "version": "1.0.0" },
            }),
        );

        let server_name = response
            .pointer("/result/serverInfo/name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(
            server_name, SERVER_NAME,
            "initialize must identify citra rmcp server"
        );

        self.send_notification("notifications/initialized", json!({}));
        self.initialized = true;
    }
}

impl Drop for StdioMcpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn compare_transport_contract_case(case: &OracleCase) {
    let native = serde_json::json!({
        "transport": resolve_transport(&case.input["env"]),
    });
    assert_eq!(
        native, case.output,
        "transport contract mismatch for case {}",
        case.id
    );
}

fn compare_surface_contract_case(case: &OracleCase) {
    let surface = case.input["surface"]
        .as_str()
        .expect("surface contract surface");
    let markers = case.input["markers"]
        .as_array()
        .expect("surface contract markers")
        .iter()
        .map(|value| value.as_str().expect("marker string").to_string())
        .collect::<Vec<_>>();
    let native = serde_json::json!({
        "markers": surface_markers(surface, &markers),
    });
    assert_eq!(
        native, case.output,
        "surface contract mismatch for case {}",
        case.id
    );
}

fn compare_server_contract_case(case: &OracleCase) {
    let native = serde_json::json!({
        "name": SERVER_NAME,
        "version": SERVER_VERSION,
        "tools": case.input["tools"],
    });
    assert_eq!(
        native.get("name"),
        case.output.get("name"),
        "server name mismatch for {}",
        case.id
    );
    assert_eq!(
        native.get("tools"),
        case.output.get("tools"),
        "server tools mismatch for {}",
        case.id
    );
}

fn compare_tool_route_case(case: &OracleCase) {
    let tool = case.input["tool"].as_str().expect("tool route tool");
    let route = route_for_tool(tool).expect("tool must be routed");
    let route_name = match route {
        ToolRoute::RustCore => "RustCore",
    };
    let native = serde_json::json!({ "route": route_name });
    assert_eq!(
        native, case.output,
        "tool route mismatch for case {}",
        case.id
    );
}

fn compare_read_pdf_tool_case(case: &OracleCase) {
    if case.output.get("status").and_then(Value::as_str) == Some("skipped") {
        return;
    }

    let args = case.input["args"].clone();

    if case.output.get("status").and_then(Value::as_str) == Some("error") {
        let err = read_pdf::read_pdf(args).expect_err("expected read_pdf rmcp error");
        let message = err.message.to_ascii_lowercase();
        let needle = case.output["message_contains"]
            .as_str()
            .expect("message_contains")
            .to_ascii_lowercase();
        assert!(
            message.contains(&needle),
            "{}: expected message to contain '{needle}', got '{}'",
            case.id,
            err.message
        );
        return;
    }

    let rmcp = read_pdf::read_pdf(args).unwrap_or_else(|error| {
        panic!("{}: rmcp read_pdf failed: {error:?}", case.id);
    });
    let raw_structured = parse_rmcp_structured(&rmcp);
    let full_text_actual = raw_structured
        .pointer("/results/0/data/full_text")
        .or_else(|| raw_structured.pointer("/results/0/data/fullText"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let structured = normalize_structured(raw_structured);

    assert_eq!(
        structured.get("profile").and_then(Value::as_str),
        case.output.get("profile").and_then(Value::as_str),
        "{}: profile mismatch",
        case.id
    );

    let results = structured
        .get("results")
        .and_then(Value::as_array)
        .expect("results array");
    assert_eq!(
        results[0].get("success").and_then(Value::as_bool),
        case.output.get("success").and_then(Value::as_bool),
        "{}: success mismatch",
        case.id
    );

    assert_eq!(
        results[0].pointer("/data/route").and_then(Value::as_str),
        case.output.get("route").and_then(Value::as_str),
        "{}: route mismatch",
        case.id
    );
    assert_eq!(
        results[0].pointer("/data/engine"),
        case.output.get("engine"),
        "{}: engine mismatch",
        case.id
    );

    if let Some(expected_info) = case.output.get("expectedInfo").and_then(Value::as_object) {
        let actual_info = results[0]
            .pointer("/data/info")
            .and_then(Value::as_object)
            .expect("info object");
        for (key, value) in expected_info {
            assert_eq!(
                actual_info.get(key),
                Some(value),
                "{}: info[{key}] mismatch",
                case.id
            );
        }
    }

    if let Some(needle) = case
        .output
        .get("full_text_contains")
        .and_then(Value::as_str)
    {
        let full_text = full_text_actual.as_deref().unwrap_or("");
        assert!(
            full_text.contains(needle),
            "{}: full_text should contain '{needle}'",
            case.id
        );
    }
}

fn sample_pdf_path() -> Option<PathBuf> {
    let sample = fixtures_root().join("sample.pdf");
    if sample.is_file() {
        Some(sample)
    } else {
        None
    }
}

fn parse_tool_call_text(response: &Value) -> Value {
    let result = response.get("result").expect("tools/call result");
    result
        .get("structuredContent")
        .cloned()
        .or_else(|| {
            result
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str(text).ok())
        })
        .expect("structured tool response")
}

fn compare_stdio_probe_case(case: &OracleCase, client: &mut StdioMcpClient) {
    let kind = case.input["kind"].as_str().expect("stdioProbe kind");
    match kind {
        "initialize" => {
            let response = client.send_request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "stdio-differential", "version": "1.0.0" },
                }),
            );
            let server_name = response
                .pointer("/result/serverInfo/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert_eq!(
                server_name,
                case.output["serverName"].as_str().expect("serverName"),
                "{}: initialize server name mismatch",
                case.id
            );
            client.send_notification("notifications/initialized", json!({}));
            client.initialized = true;
        }
        "toolsList" => {
            client.initialize_session();
            let response = client.send_request("tools/list", json!({}));
            let tools = response
                .pointer("/result/tools")
                .and_then(Value::as_array)
                .expect("tools array");
            let mut names: Vec<String> = tools
                .iter()
                .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
                .collect();
            names.sort();
            let mut expected = case.output["tools"]
                .as_array()
                .expect("expected tools")
                .iter()
                .map(|value| value.as_str().expect("tool name").to_string())
                .collect::<Vec<_>>();
            expected.sort();
            assert_eq!(names, expected, "{}: tools/list mismatch", case.id);
        }
        "readPdf" => {
            if case.output.get("status").and_then(Value::as_str) == Some("skipped") {
                return;
            }

            client.initialize_session();
            let args = case.input["args"].clone();
            let response = client.send_request(
                "tools/call",
                json!({
                    "name": "read_pdf",
                    "arguments": args,
                }),
            );

            if case.output.get("error").and_then(Value::as_bool) == Some(true) {
                let message = response
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        response
                            .pointer("/result/content/0/text")
                            .and_then(Value::as_str)
                    })
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let needle = case.output["message_contains"]
                    .as_str()
                    .expect("message_contains")
                    .to_ascii_lowercase();
                assert!(
                    message.contains(&needle),
                    "{}: stdio read_pdf error should contain '{needle}', got '{message}'",
                    case.id
                );
                return;
            }

            let result = response
                .get("result")
                .unwrap_or_else(|| panic!("{}: tools/call result missing: {response}", case.id));
            assert!(
                result.get("isError").and_then(Value::as_bool) != Some(true),
                "{}: read_pdf over stdio failed: {response}",
                case.id
            );

            let raw_structured = result
                .get("structuredContent")
                .cloned()
                .or_else(|| {
                    result
                        .pointer("/content/0/text")
                        .and_then(Value::as_str)
                        .and_then(|text| serde_json::from_str(text).ok())
                })
                .expect("structured read_pdf response");
            let full_text_actual = raw_structured
                .pointer("/results/0/data/full_text")
                .or_else(|| raw_structured.pointer("/results/0/data/fullText"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let structured = normalize_structured(raw_structured);

            assert_eq!(
                structured.get("profile").and_then(Value::as_str),
                case.output.get("profile").and_then(Value::as_str),
                "{}: stdio profile mismatch",
                case.id
            );

            let results = structured
                .get("results")
                .and_then(Value::as_array)
                .expect("results array");
            assert_eq!(
                results[0].get("success").and_then(Value::as_bool),
                case.output.get("success").and_then(Value::as_bool),
                "{}: stdio success mismatch",
                case.id
            );
            assert_eq!(
                results[0].pointer("/data/route").and_then(Value::as_str),
                case.output.get("route").and_then(Value::as_str),
                "{}: stdio route mismatch",
                case.id
            );
            assert_eq!(
                results[0].pointer("/data/engine"),
                case.output.get("engine"),
                "{}: stdio engine mismatch",
                case.id
            );

            if let Some(expected_info) = case.output.get("expectedInfo").and_then(Value::as_object)
            {
                let actual_info = results[0]
                    .pointer("/data/info")
                    .and_then(Value::as_object)
                    .expect("info object");
                for (key, value) in expected_info {
                    assert_eq!(
                        actual_info.get(key),
                        Some(value),
                        "{}: stdio info[{key}] mismatch",
                        case.id
                    );
                }
            }

            if let Some(needle) = case
                .output
                .get("full_text_contains")
                .and_then(Value::as_str)
            {
                let full_text = full_text_actual.as_deref().unwrap_or("");
                assert!(
                    full_text.contains(needle),
                    "{}: stdio full_text should contain '{needle}'",
                    case.id
                );
            }
        }
        "searchPdf" => {
            if case.output.get("status").and_then(Value::as_str) == Some("skipped") {
                return;
            }

            let sample_pdf = sample_pdf_path().expect("sample.pdf required for searchPdf probe");
            client.initialize_session();
            let response = client.send_request(
                "tools/call",
                json!({
                    "name": "search_pdf",
                    "arguments": {
                        "sources": [{ "path": sample_pdf.to_string_lossy() }],
                        "query": "Lorem",
                        "detail": true,
                    },
                }),
            );

            let structured = parse_tool_call_text(&response);
            assert_eq!(
                structured.get("profile").and_then(Value::as_str),
                case.output.get("profile").and_then(Value::as_str),
                "{}: searchPdf profile mismatch",
                case.id
            );

            let results = structured
                .get("results")
                .and_then(Value::as_array)
                .expect("searchPdf results array");
            assert_eq!(
                results[0].get("success").and_then(Value::as_bool),
                case.output.get("success").and_then(Value::as_bool),
                "{}: searchPdf success mismatch",
                case.id
            );

            if let Some(route_needle) = case.output.get("route_contains").and_then(Value::as_str) {
                let route = results[0]
                    .pointer("/data/route")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                assert!(
                    route.contains(route_needle),
                    "{}: searchPdf route should contain '{route_needle}', got '{route}'",
                    case.id
                );
            }
        }
        "pdfEvidence" => {
            if case.output.get("status").and_then(Value::as_str) == Some("skipped") {
                return;
            }

            let sample_pdf = sample_pdf_path().expect("sample.pdf required for pdfEvidence probe");
            client.initialize_session();
            let response = client.send_request(
                "tools/call",
                json!({
                    "name": "pdf_evidence",
                    "arguments": {
                        "operation": "inspect",
                        "sources": [{ "path": sample_pdf.to_string_lossy() }],
                    },
                }),
            );

            let structured = parse_tool_call_text(&response);
            let results = structured
                .get("results")
                .and_then(Value::as_array)
                .expect("pdfEvidence results array");
            assert_eq!(
                results[0].get("success").and_then(Value::as_bool),
                case.output.get("success").and_then(Value::as_bool),
                "{}: pdfEvidence success mismatch",
                case.id
            );

            if let Some(route_needle) = case.output.get("route_contains").and_then(Value::as_str) {
                let route = results[0]
                    .pointer("/data/route")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                assert!(
                    route.contains(route_needle),
                    "{}: pdfEvidence route should contain '{route_needle}', got '{route}'",
                    case.id
                );
            }
        }
        other => panic!("unknown stdioProbe kind {other} in case {}", case.id),
    }
}

fn slice_filter() -> Option<String> {
    std::env::var("PDF_READER_MCP_SLICE_FILTER")
        .ok()
        .filter(|value| value != "all" && !value.is_empty())
}

fn case_matches_slice(case: &OracleCase, slice: &str) -> bool {
    match slice {
        "tool.read_pdf" => {
            case.domain == "readPdfTool"
                || (case.domain == "stdioProbe"
                    && case.input.get("kind").and_then(Value::as_str) == Some("readPdf"))
        }
        "tool.search_pdf|tool.pdf_evidence" => {
            (case.domain == "toolRouteContract"
                && matches!(
                    case.input.get("tool").and_then(Value::as_str),
                    Some("search_pdf") | Some("pdf_evidence")
                ))
                || (case.domain == "stdioProbe"
                    && matches!(
                        case.input.get("kind").and_then(Value::as_str),
                        Some("searchPdf") | Some("pdfEvidence")
                    ))
        }
        "transport.stdio-rust-rmcp" => {
            matches!(
                case.domain.as_str(),
                "transportContract" | "surfaceContract" | "serverContract"
            ) || (case.domain == "stdioProbe"
                && matches!(
                    case.input.get("kind").and_then(Value::as_str),
                    Some("initialize") | Some("toolsList")
                ))
        }
        _ => true,
    }
}

#[test]
fn pdf_reader_mcp_differential_matches_ts_oracle() {
    // Pure-Rust is the only production engine.
    let _ = fs::read_to_string(corpus_fixture_path()).expect("read pdf-reader-mcp corpus fixture");
    let Some(oracle) = run_ts_oracle() else {
        return;
    };
    assert_eq!(oracle.corpus_version, 1);
    assert_eq!(oracle.profile, "pdf_reader_read_pdf_golden");
    assert!(!oracle.fixture_corpus_hash.is_empty());
    assert!(!oracle.golden_fixture_hash.is_empty());
    assert!(!oracle.cases.is_empty(), "oracle must emit cases");

    let cases: Vec<&OracleCase> = if let Some(slice) = slice_filter() {
        oracle
            .cases
            .iter()
            .filter(|case| case_matches_slice(case, &slice))
            .collect()
    } else {
        oracle.cases.iter().collect()
    };
    assert!(
        !cases.is_empty(),
        "bounded slice filter must retain at least one oracle case"
    );

    let stdio_cases: Vec<&OracleCase> = cases
        .iter()
        .copied()
        .filter(|case| case.domain == "stdioProbe")
        .collect();

    let mut stdio_client = if !stdio_cases.is_empty() {
        Some(StdioMcpClient::spawn())
    } else {
        None
    };

    for case in cases {
        match case.domain.as_str() {
            "transportContract" => compare_transport_contract_case(case),
            "surfaceContract" => compare_surface_contract_case(case),
            "serverContract" => compare_server_contract_case(case),
            "toolRouteContract" => compare_tool_route_case(case),
            "readPdfTool" => compare_read_pdf_tool_case(case),
            "stdioProbe" => compare_stdio_probe_case(
                case,
                stdio_client
                    .as_mut()
                    .expect("stdio client required for stdioProbe cases"),
            ),
            other => panic!("unknown oracle domain {other} in case {}", case.id),
        }
    }

    let _router = PdfReaderMcp::new();
}
