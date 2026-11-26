//! Region-analysis provider configuration from environment variables: command, HTTP and named presets.

mod http;
mod presets;

use http::{command_provider_config, http_provider_config, parse_http_headers};
use presets::{provider_env, resolve_preset};

#[derive(Clone, Debug)]
pub(super) enum ProviderConfig {
    Command {
        command: String,
        args_template: Vec<String>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        preset: Option<String>,
        model: Option<String>,
    },
}

impl ProviderConfig {
    pub(super) fn provider_label(&self) -> &'static str {
        match self {
            Self::Command { .. } => "command",
            Self::Http { .. } => "http",
        }
    }

    pub(super) fn engine_label(&self) -> &'static str {
        match self {
            Self::Command { .. } => "external-command",
            Self::Http { .. } => "external-http",
        }
    }
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

pub(super) fn provider_config_from(mut values: ProviderEnv) -> Result<ProviderConfig, String> {
    let command = values
        .command_value
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(command) = command {
        return command_provider_config(command, values.args_value);
    }

    let headers = parse_http_headers(values.headers_value.take())?;
    let preset = values
        .preset_value
        .take()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if let Some(preset) = preset {
        return resolve_preset(preset, headers, values);
    }

    let http_url = values
        .http_value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(url) = http_url {
        return http_provider_config(url, headers, None, None);
    }

    Err("Region analysis provider is not configured. Set MCP_PDF_REGION_ANALYSIS_COMMAND, MCP_PDF_REGION_ANALYSIS_HTTP_URL, or MCP_PDF_REGION_ANALYSIS_PRESET=ollama/openai-compatible/lmstudio/llamacpp to enable analyze_regions.".into())
}

pub(super) fn provider_config() -> Result<ProviderConfig, String> {
    provider_config_from(provider_env())
}
