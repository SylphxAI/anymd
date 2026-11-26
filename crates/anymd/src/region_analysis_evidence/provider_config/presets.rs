//! Environment capture and named provider preset resolution.

use std::env;

use super::http::http_provider_config;
use super::{ProviderConfig, ProviderEnv};

const COMMAND_ENV: &str = "MCP_PDF_REGION_ANALYSIS_COMMAND";
const ARGS_ENV: &str = "MCP_PDF_REGION_ANALYSIS_ARGS_JSON";
const HTTP_ENV: &str = "MCP_PDF_REGION_ANALYSIS_HTTP_URL";
const HTTP_HEADERS_ENV: &str = "MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON";
const PRESET_ENV: &str = "MCP_PDF_REGION_ANALYSIS_PRESET";
const OLLAMA_URL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OLLAMA_URL";
const OLLAMA_MODEL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OLLAMA_MODEL";
const OPENAI_URL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OPENAI_URL";
const OPENAI_MODEL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OPENAI_MODEL";
const OPENAI_API_KEY_ENV: &str = "MCP_PDF_REGION_ANALYSIS_OPENAI_API_KEY";
const LMSTUDIO_URL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_LMSTUDIO_URL";
const LMSTUDIO_MODEL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_LMSTUDIO_MODEL";
const LLAMACPP_URL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_LLAMACPP_URL";
const LLAMACPP_MODEL_ENV: &str = "MCP_PDF_REGION_ANALYSIS_LLAMACPP_MODEL";
const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434/api/generate";
const DEFAULT_LMSTUDIO_URL: &str = "http://127.0.0.1:1234/v1/chat/completions";
const DEFAULT_LLAMACPP_URL: &str = "http://127.0.0.1:8080/v1/chat/completions";

pub(super) fn provider_env() -> ProviderEnv {
    ProviderEnv {
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
    }
}

pub(super) fn resolve_preset(
    preset: String,
    headers: Vec<(String, String)>,
    values: ProviderEnv,
) -> Result<ProviderConfig, String> {
    let ProviderEnv {
        ollama_url,
        ollama_model,
        openai_url,
        openai_model,
        openai_api_key,
        lmstudio_url,
        lmstudio_model,
        llamacpp_url,
        llamacpp_model,
        ..
    } = values;
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
            http_provider_config(url, headers, Some(preset), Some(model))
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
            http_provider_config(url, headers, Some(preset), Some(model))
        }
        _ => {
            Err(
                "Unsupported MCP_PDF_REGION_ANALYSIS_PRESET. Supported values: ollama, openai-compatible, lmstudio, llamacpp."
                    .into(),
            )
        }
    }
}
