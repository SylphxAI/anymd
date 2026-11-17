//! Region-analysis provider configuration from environment variables: command, HTTP and named presets.

use super::*;

pub(super) fn not_configured_message() -> String {
    "Region analysis provider is not configured. Set MCP_PDF_REGION_ANALYSIS_COMMAND, MCP_PDF_REGION_ANALYSIS_HTTP_URL, or MCP_PDF_REGION_ANALYSIS_PRESET=ollama/openai-compatible/lmstudio/llamacpp to enable analyze_regions.".into()
}

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

pub(super) fn validate_http_url(url: &str, invalid_message: &str) -> Result<(), String> {
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

#[derive(Default)]
pub(super) struct ProviderEnv {
    pub(super) command_value: Option<String>,
    pub(super) args_value: Option<String>,
    pub(super) http_value: Option<String>,
    pub(super) headers_value: Option<String>,
    pub(super) preset_value: Option<String>,
    pub(super) ollama_url: Option<String>,
    pub(super) ollama_model: Option<String>,
    pub(super) openai_url: Option<String>,
    pub(super) openai_model: Option<String>,
    pub(super) openai_api_key: Option<String>,
    pub(super) lmstudio_url: Option<String>,
    pub(super) lmstudio_model: Option<String>,
    pub(super) llamacpp_url: Option<String>,
    pub(super) llamacpp_model: Option<String>,
}

pub(super) fn provider_config_from(values: ProviderEnv) -> Result<ProviderConfig, String> {
    let ProviderEnv {
        command_value,
        args_value,
        http_value,
        headers_value,
        preset_value,
        ollama_url,
        ollama_model,
        openai_url,
        openai_model,
        openai_api_key,
        lmstudio_url,
        lmstudio_model,
        llamacpp_url,
        llamacpp_model,
    } = values;
    let command = command_value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(command) = command {
        return command_provider_config(command, args_value);
    }

    let headers = parse_http_headers(headers_value)?;
    let preset = preset_value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if let Some(preset) = preset.clone() {
        match preset.as_str() {
            "ollama" => {
                let model = ollama_model
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        "MCP_PDF_REGION_ANALYSIS_OLLAMA_MODEL is required when MCP_PDF_REGION_ANALYSIS_PRESET=ollama."
                            .to_string()
                    })?;
                let url = ollama_url
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| DEFAULT_OLLAMA_URL.into());
                return http_provider_config(url, headers, Some(preset), Some(model));
            }
            "openai-compatible" | "lmstudio" | "llamacpp" => {
                let (model_env, url_env, default_url, model_value, url_value) =
                    match preset.as_str() {
                        "openai-compatible" => (
                            OPENAI_MODEL_ENV,
                            OPENAI_URL_ENV,
                            None,
                            openai_model,
                            openai_url,
                        ),
                        "lmstudio" => (
                            LMSTUDIO_MODEL_ENV,
                            LMSTUDIO_URL_ENV,
                            Some(DEFAULT_LMSTUDIO_URL),
                            lmstudio_model,
                            lmstudio_url,
                        ),
                        _ => (
                            LLAMACPP_MODEL_ENV,
                            LLAMACPP_URL_ENV,
                            Some(DEFAULT_LLAMACPP_URL),
                            llamacpp_model,
                            llamacpp_url,
                        ),
                    };
                let model = model_value
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        format!(
                            "{model_env} is required when MCP_PDF_REGION_ANALYSIS_PRESET={preset}."
                        )
                    })?;
                let url = url_value
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .or_else(|| default_url.map(str::to_string))
                    .ok_or_else(|| {
                        if preset == "openai-compatible" {
                            "MCP_PDF_REGION_ANALYSIS_OPENAI_URL is required when MCP_PDF_REGION_ANALYSIS_PRESET=openai-compatible.".into()
                        } else {
                            format!("{url_env} is required when MCP_PDF_REGION_ANALYSIS_PRESET={preset}.")
                        }
                    })?;
                let mut headers = headers;
                if let Some(api_key) = openai_api_key
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                {
                    headers.retain(|(key, _)| !key.eq_ignore_ascii_case("authorization"));
                    headers.push(("Authorization".into(), format!("Bearer {api_key}")));
                }
                return http_provider_config(url, headers, Some(preset), Some(model));
            }
            _ => {
                return Err(
                    "Unsupported MCP_PDF_REGION_ANALYSIS_PRESET. Supported values: ollama, openai-compatible, lmstudio, llamacpp."
                        .into(),
                );
            }
        }
    }

    let http_url = http_value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(url) = http_url {
        return http_provider_config(url, headers, None, None);
    }

    Err(not_configured_message())
}

pub(super) fn provider_config() -> Result<ProviderConfig, String> {
    provider_config_from(ProviderEnv {
        command_value: env::var(COMMAND_ENV).ok(),
        args_value: env::var(ARGS_ENV).ok(),
        http_value: env::var(HTTP_ENV).ok(),
        headers_value: env::var(HTTP_HEADERS_ENV).ok(),
        preset_value: env::var(PRESET_ENV).ok(),
        ollama_url: env::var(OLLAMA_URL_ENV).ok(),
        ollama_model: env::var(OLLAMA_MODEL_ENV).ok(),
        openai_url: env::var(OPENAI_URL_ENV).ok(),
        openai_model: env::var(OPENAI_MODEL_ENV).ok(),
        openai_api_key: env::var(OPENAI_API_KEY_ENV).ok(),
        lmstudio_url: env::var(LMSTUDIO_URL_ENV).ok(),
        lmstudio_model: env::var(LMSTUDIO_MODEL_ENV).ok(),
        llamacpp_url: env::var(LLAMACPP_URL_ENV).ok(),
        llamacpp_model: env::var(LLAMACPP_MODEL_ENV).ok(),
    })
}
