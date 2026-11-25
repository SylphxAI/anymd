use rmcp::handler::server::router::tool::ToolRouter;
use serde_json::Value;

use super::PdfReaderMcp;

pub(super) fn omit_absent_optional_fields(value: Value) -> Value {
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
pub(super) fn is_standard_schema_format(format: &str) -> bool {
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

pub(super) fn sanitize_schema_formats(object: &mut serde_json::Map<String, Value>) {
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

pub(super) fn sanitized_tools(router: &ToolRouter<PdfReaderMcp>) -> Vec<rmcp::model::Tool> {
    let mut tools = router.list_all();
    for tool in &mut tools {
        sanitize_schema_formats(std::sync::Arc::make_mut(&mut tool.input_schema));
        if let Some(output_schema) = &mut tool.output_schema {
            sanitize_schema_formats(std::sync::Arc::make_mut(output_schema));
        }
    }
    tools
}
