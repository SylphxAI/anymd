//! Command arguments and HTTP transport configuration.

use serde_json::Value;

use super::ProviderConfig;

pub(super) fn parse_http_headers(raw: Option<String>) -> Result<Vec<(String, String)>, String> {
    let Some(raw) = raw
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(Vec::new());
    };
    let value: Value = serde_json::from_str(&raw).map_err(|_| {
        "MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON must be a JSON object with string keys and string values.".to_string()
    })?;
    let object = value.as_object().ok_or_else(|| {
        "MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON must be a JSON object with string keys and string values.".to_string()
    })?;
    let mut headers = Vec::with_capacity(object.len());
    for (key, value) in object {
        let key = key.trim();
        let Some(header_value) = value.as_str().map(str::trim) else {
            return Err(
                "MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON must be a JSON object with string keys and string values."
                    .into(),
            );
        };
        if key.is_empty() {
            return Err(
                "MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON must be a JSON object with string keys and string values."
                    .into(),
            );
        }
        headers.push((key.to_string(), header_value.to_string()));
    }
    Ok(headers)
}

fn validate_http_url(url: &str, invalid_message: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(invalid_message.to_string());
    }
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or("");
    let host = without_scheme.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.split('@').next_back().unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(host, _)| host);
    if host.is_empty() || host == "[" {
        return Err(invalid_message.to_string());
    }
    Ok(())
}

pub(super) fn command_provider_config(
    command: String,
    args_value: Option<String>,
) -> Result<ProviderConfig, String> {
    let args_template = match args_value {
        None => vec!["{input}".into()],
        Some(raw) => {
            let value: Value = serde_json::from_str(&raw).map_err(|_| {
                "MCP_PDF_REGION_ANALYSIS_ARGS_JSON must be a JSON string array.".to_string()
            })?;
            let values = value.as_array().ok_or_else(|| {
                "MCP_PDF_REGION_ANALYSIS_ARGS_JSON must be a JSON string array.".to_string()
            })?;
            let args = values
                .iter()
                .map(|entry| {
                    entry.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                        "MCP_PDF_REGION_ANALYSIS_ARGS_JSON must be a JSON string array.".to_string()
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if !args.iter().any(|arg| arg.contains("{input}")) {
                return Err("MCP_PDF_REGION_ANALYSIS_ARGS_JSON must include the {input} placeholder so the provider receives the cropped region image.".into());
            }
            args
        }
    };
    Ok(ProviderConfig::Command {
        command,
        args_template,
    })
}

pub(super) fn http_provider_config(
    url: String,
    headers: Vec<(String, String)>,
    preset: Option<String>,
    model: Option<String>,
) -> Result<ProviderConfig, String> {
    validate_http_url(
        &url,
        if preset.as_deref() == Some("openai-compatible") {
            "MCP_PDF_REGION_ANALYSIS_OPENAI_URL must be a valid URL."
        } else if preset.as_deref() == Some("ollama") {
            "MCP_PDF_REGION_ANALYSIS_OLLAMA_URL must be a valid URL."
        } else if preset.as_deref() == Some("lmstudio") {
            "MCP_PDF_REGION_ANALYSIS_LMSTUDIO_URL must be a valid URL."
        } else if preset.as_deref() == Some("llamacpp") {
            "MCP_PDF_REGION_ANALYSIS_LLAMACPP_URL must be a valid URL."
        } else {
            "MCP_PDF_REGION_ANALYSIS_HTTP_URL must be a valid URL."
        },
    )?;
    Ok(ProviderConfig::Http {
        url,
        headers,
        preset,
        model,
    })
}
