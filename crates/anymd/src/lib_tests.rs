use super::schema_sanitize::{is_standard_schema_format, sanitize_schema_formats, sanitized_tools};
use super::PdfReaderMcp;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;
use std::path::PathBuf;

fn locked() -> PdfReaderMcp {
    PdfReaderMcp::new().with_pro_check(|| false)
}

fn unlocked() -> PdfReaderMcp {
    PdfReaderMcp::new().with_pro_check(|| true)
}

fn result_text(result: &rmcp::model::CallToolResult) -> String {
    serde_json::to_string(&result.content).expect("content serializes")
}

const MISSING: &str = "/nonexistent/anymd-pro-gate-probe";

fn inspect_args(operation: &str, extra: Value) -> crate::schema::InspectArgs {
    let mut value = serde_json::json!({
        "operation": operation,
        "sources": [{"path": MISSING}],
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value).expect("valid inspect args")
}

#[tokio::test]
async fn pro_operations_without_a_licence_return_the_message_and_do_no_work() {
    let video = serde_json::json!({"timeline": {"end_ms": 1000}, "timestamps_ms": [0]});
    let cite = serde_json::json!({
        "citations": [{"quote": "a", "page": 1, "bounding_box": {"left": 0, "bottom": 0, "right": 1, "top": 1}}]
    });
    for (operation, feature, extra) in [
        ("video_timeline", "Video timeline", video.clone()),
        ("render_frame", "Video frame rendering", video),
        ("cite_check", "Cite-check", cite),
    ] {
        let result = locked()
            .inspect(Parameters(inspect_args(operation, extra)))
            .await
            .expect("an unlicensed Pro call is a normal result, not a protocol error");
        assert_ne!(result.is_error, Some(true), "{operation}");
        assert_eq!(
            result_text(&result),
            format!(
                "[{{\"type\":\"text\",\"text\":\"{feature} is part of anymd Pro ($29 one-time offline licence). See current pricing or buy it with `anymd pro buy`. Learn more: https://anymd.sylphx.com/pricing\"}}]"
            ),
            "{operation}"
        );
    }
}

#[tokio::test]
async fn timeline_option_on_read_and_outline_is_gated_without_a_licence() {
    let read: crate::schema::ReadArgs = serde_json::from_value(
        serde_json::json!({"source": MISSING, "timeline": {"end_ms": 1000}}),
    )
    .expect("valid read args");
    let result = locked()
        .read(Parameters(read))
        .await
        .expect("normal result");
    assert_ne!(result.is_error, Some(true));
    assert!(result_text(&result).contains("is part of anymd Pro"));
    let outline: crate::schema::OutlineArgs = serde_json::from_value(
        serde_json::json!({"source": MISSING, "timeline": {"end_ms": 1000}}),
    )
    .expect("valid outline args");
    let result = locked()
        .outline(Parameters(outline))
        .await
        .expect("normal result");
    assert!(result_text(&result).contains("is part of anymd Pro"));
}

#[tokio::test]
async fn licensed_pro_operations_proceed_past_the_gate() {
    let cite = serde_json::json!({
        "citations": [{"quote": "a", "page": 1, "bounding_box": {"left": 0, "bottom": 0, "right": 1, "top": 1}}]
    });
    let video = serde_json::json!({"timeline": {"end_ms": 1000}, "timestamps_ms": [0]});
    for (operation, extra) in [("cite_check", cite), ("video_timeline", video)] {
        let outcome = unlocked()
            .inspect(Parameters(inspect_args(operation, extra)))
            .await;
        let text = match &outcome {
            Ok(result) => result_text(result),
            Err(error) => error.message.to_string(),
        };
        assert!(
            !text.contains("is part of anymd Pro"),
            "{operation}: {text}"
        );
    }
}

#[tokio::test]
async fn free_operations_never_hit_the_pro_gate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("note.md");
    std::fs::write(&path, "# Title\n\nhello world\n").expect("write");
    let server = locked();
    let read: crate::schema::ReadArgs =
        serde_json::from_value(serde_json::json!({"source": path})).unwrap();
    let read = server.read(Parameters(read)).await.expect("read");
    assert!(result_text(&read).contains("hello world"));
    let outline: crate::schema::OutlineArgs =
        serde_json::from_value(serde_json::json!({"source": path})).unwrap();
    let outline = server.outline(Parameters(outline)).await.expect("outline");
    assert!(result_text(&outline).contains("Title"));
    let search: crate::schema::SearchArgs =
        serde_json::from_value(serde_json::json!({"query": "hello", "sources": [path]})).unwrap();
    let search = server.search(Parameters(search)).await.expect("search");
    assert!(!result_text(&search).contains("is part of anymd Pro"));
    for operation in ["inspect", "structure", "render_page"] {
        let outcome = server
            .inspect(Parameters(inspect_args(operation, serde_json::json!({}))))
            .await;
        let text = match &outcome {
            Ok(result) => result_text(result),
            Err(error) => error.message.to_string(),
        };
        assert!(!text.contains("is part of anymd Pro"), "{operation}");
    }
}

#[test]
fn exposes_four_obvious_tools_and_keeps_legacy_names_callable() {
    let tools = PdfReaderMcp::new().tool_router.list_all();
    let mut names: Vec<_> = tools.iter().map(|tool| tool.name.to_string()).collect();
    names.sort();
    assert_eq!(names, ["inspect", "outline", "read", "search"]);
    for legacy in super::LEGACY_TOOL_NAMES {
        assert!(
            !names.contains(&legacy.to_string()),
            "{legacy} must not be listed"
        );
    }
}

#[test]
fn tools_list_exposes_typed_object_schemas_not_empty_value() {
    let tools = PdfReaderMcp::new().tool_router.list_all();
    for tool in tools {
        let schema_value = serde_json::to_value(&tool.input_schema).expect("schema json");
        assert_eq!(
            schema_value.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "tool {} schema type must be object, got {schema_value}",
            tool.name
        );
        let props = schema_value
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("properties object");
        let required: &[&str] = match tool.name.as_ref() {
            "read" => &["source", "pages", "max_tokens", "cursor", "node"],
            "outline" => &["source", "format"],
            "search" => &["query", "sources", "mode"],
            "inspect" => &["operation", "sources"],
            other => panic!("unexpected tool {other}"),
        };
        for key in required {
            assert!(
                props.contains_key(*key),
                "tool {} must document {key}",
                tool.name
            );
        }
    }
}

// A strict client checks key presence before tools/call. Nullable values
// alone do not allow omission: required must agree with serde's decoder.
#[test]
fn tools_list_required_matches_argument_omission_for_every_tool() {
    for tool in sanitized_tools(&PdfReaderMcp::new().tool_router) {
        let schema = serde_json::to_value(&tool.input_schema).expect("schema json");
        let (minimal, expected_required) = match tool.name.as_ref() {
            "read" | "outline" => (serde_json::json!({"source": "report.pdf"}), vec!["source"]),
            "search" => (serde_json::json!({"query": "needle"}), vec!["query"]),
            "inspect" => (
                serde_json::json!({"operation": "inspect", "sources": [{"path": "report.pdf"}]}),
                vec!["operation", "sources"],
            ),
            other => panic!("add an omission contract for tool {other}"),
        };
        let mut required: Vec<_> = schema["required"]
            .as_array()
            .expect("required keys")
            .iter()
            .map(|key| key.as_str().expect("required key string"))
            .collect();
        required.sort_unstable();
        assert_eq!(
            required, expected_required,
            "tool {} required keys",
            tool.name
        );
        assert!(tool_arguments_decode(&tool.name, minimal.clone()));

        for key in schema["properties"].as_object().expect("properties").keys() {
            let mut omitted = minimal.clone();
            omitted
                .as_object_mut()
                .expect("arguments object")
                .remove(key);
            let strict_client_accepts = required.iter().all(|key| omitted.get(*key).is_some());
            assert_eq!(
                strict_client_accepts,
                tool_arguments_decode(&tool.name, omitted),
                "tool {} disagrees with strict client when {key} is omitted",
                tool.name
            );
        }
    }
}

fn tool_arguments_decode(name: &str, value: Value) -> bool {
    match name {
        "read" => serde_json::from_value::<crate::schema::ReadArgs>(value).is_ok(),
        "search" => serde_json::from_value::<crate::schema::SearchArgs>(value).is_ok(),
        "outline" => serde_json::from_value::<crate::schema::OutlineArgs>(value).is_ok(),
        "inspect" => serde_json::from_value::<crate::schema::InspectArgs>(value).is_ok(),
        other => panic!("unknown tool {other}"),
    }
}

#[test]
fn tools_list_optional_overrides_keep_types_enums_and_bounds() {
    for tool in sanitized_tools(&PdfReaderMcp::new().tool_router) {
        let schema = serde_json::to_value(&tool.input_schema).expect("schema json");
        let properties = &schema["properties"];
        match tool.name.as_ref() {
            "read" => {
                for key in ["transcript", "download_asr_model"] {
                    assert_eq!(
                        properties[key]["type"],
                        serde_json::json!(["boolean", "null"])
                    );
                }
                assert_eq!(
                    properties["images"]["type"],
                    serde_json::json!(["string", "null"])
                );
                assert_eq!(
                    properties["images"]["enum"],
                    serde_json::json!(["refs", "none", null])
                );
                assert_eq!(
                    properties["revisions"]["type"],
                    serde_json::json!(["string", "null"])
                );
                assert_eq!(
                    properties["revisions"]["enum"],
                    serde_json::json!(["markup", "accept", "reject", null])
                );
                assert_eq!(properties["source"]["minLength"], 1);
                assert_eq!(properties["max_tokens"]["minimum"], 500);
            }
            "search" => {
                for key in ["case_sensitive", "whole_word"] {
                    assert_eq!(
                        properties[key]["type"],
                        serde_json::json!(["boolean", "null"])
                    );
                }
                assert_eq!(properties["query"]["minLength"], 1);
                assert_eq!(properties["max_results"]["minimum"], 1);
                assert_eq!(properties["max_results"]["maximum"], 500);
            }
            "inspect" => {
                assert_eq!(properties["scale"]["minimum"], 0.25);
                assert_eq!(properties["scale"]["maximum"], 4.0);
            }
            "outline" => {}
            other => panic!("unknown tool {other}"),
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

#[tokio::test]
async fn read_pdf_contains_malformed_cff_custom_encoding_panic() {
    // Regression for SylphxAI/citra#660: the CFF Custom encoding
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
    // Regression for SylphxAI/citra#675: a malformed inline
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
    assert!(
        !result.content.is_empty(),
        "expected a structured read_pdf result"
    );
}

#[tokio::test]
async fn search_pdf_handles_malformed_cid_cmap_fixture_without_aborting() {
    // Regression for SylphxAI/citra#608: a pdfTeX ToUnicode CMap
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

fn allowed_sample(temp: &tempfile::TempDir) -> std::path::PathBuf {
    let allowed = temp.path().join("allowed");
    std::fs::create_dir(&allowed).expect("allowed root");
    let sample = allowed.join("sample.pdf");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf"),
        &sample,
    )
    .expect("copy fixture");
    sample
}

fn outside_sample() -> std::path::PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/sample.pdf")
        .canonicalize()
        .expect("outside fixture")
}

#[tokio::test]
async fn every_dispatched_tool_refuses_a_path_outside_the_allowlist() {
    let temp = tempfile::tempdir().expect("tempdir");
    let allowed = temp.path().join("allowed");
    let sample = allowed_sample(&temp);
    let second = {
        let mut path = sample.clone();
        path.set_file_name("second.pdf");
        std::fs::copy(&sample, &path).expect("copy second fixture");
        path
    };
    let outside = outside_sample();
    let policy = crate::source_access::SourceAccessPolicy::restricted_for_test(
        temp.path().to_path_buf(),
        &allowed,
    )
    .expect("restricted policy");
    let server = PdfReaderMcp::with_source_access(policy);
    fn request(name: &str, arguments: &Value) -> rmcp::model::CallToolRequestParams {
        rmcp::model::CallToolRequestParams::new(name.to_string())
            .with_arguments(arguments.as_object().cloned().unwrap_or_default())
    }
    async fn denied(server: &PdfReaderMcp, name: &str, arguments: Value) -> String {
        match server.call_legacy(&request(name, &arguments)).await {
            Some(Err(error)) => error.message.to_string(),
            Some(Ok(_)) => panic!("{name} served a path outside the allowlist"),
            None => panic!("{name} is not dispatched to a tool entrypoint"),
        }
    }
    async fn served(server: &PdfReaderMcp, name: &str, arguments: Value) -> Value {
        let result = server
            .call_legacy(&request(name, &arguments))
            .await
            .unwrap_or_else(|| panic!("{name} is not dispatched to a tool entrypoint"))
            .unwrap_or_else(|error| panic!("{name} rejected an allowed path: {error}"));
        result.structured_content.unwrap_or(Value::Null)
    }

    for name in ["read_pdf", "search_pdf", "pdf_evidence", "pdf_compare"] {
        assert!(
            crate::LEGACY_TOOL_NAMES.contains(&name),
            "{name} must stay dispatched"
        );
    }

    let message = denied(
        &server,
        "pdf_compare",
        serde_json::json!({"before": outside, "after": sample}),
    )
    .await;
    assert!(message.contains("Access denied"), "{message}");
    let message = denied(
        &server,
        "pdf_compare",
        serde_json::json!({"before": sample, "after": outside}),
    )
    .await;
    assert!(message.contains("Access denied"), "{message}");
    // The current compare entrypoint honours the same boundary.
    let inspect = server
        .inspect(Parameters(inspect_args(
            "compare",
            serde_json::json!({"sources": [{"path": outside}, {"path": sample}]}),
        )))
        .await
        .expect_err("inspect compare must reject outside path");
    assert!(inspect.message.contains("Access denied"));

    denied(
        &server,
        "read_pdf",
        serde_json::json!({"sources": [{"path": outside}]}),
    )
    .await;
    denied(
        &server,
        "search_pdf",
        serde_json::json!({"sources": [{"path": outside}], "query": "needle"}),
    )
    .await;
    denied(
        &server,
        "pdf_evidence",
        serde_json::json!({"operation": "inspect", "sources": [{"path": outside}]}),
    )
    .await;

    // An allowed pair stays reachable.
    let body = served(
        &server,
        "pdf_compare",
        serde_json::json!({"before": sample, "after": second, "max_file_bytes": 1048576}),
    )
    .await;
    assert!(
        body.get("error").is_none(),
        "pdf_compare reported an error for an allowed pair: {body}"
    );
}
