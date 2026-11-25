//! Dispatch region analysis to the configured HTTP or local command provider.

use serde_json::Value;
use tempfile::TempDir;

use crate::command_provider::{self, CommandInvocation, CommandRunError};

use super::provider_config::ProviderConfig;

mod http;

fn js_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn replace_placeholders(
    template: &str,
    input: &str,
    source: &str,
    region: &Value,
    languages: &[String],
) -> String {
    let box_ = &region["source_bounding_box"];
    template
        .replace("{input}", input)
        .replace("{page}", &region["page"].as_u64().unwrap_or(0).to_string())
        .replace("{source}", source)
        .replace("{region_id}", region["region_id"].as_str().unwrap_or(""))
        .replace(
            "{evidence_id}",
            region["evidence_id"].as_str().unwrap_or(""),
        )
        .replace("{left}", &js_number(box_["left"].as_f64().unwrap_or(0.0)))
        .replace(
            "{bottom}",
            &js_number(box_["bottom"].as_f64().unwrap_or(0.0)),
        )
        .replace("{right}", &js_number(box_["right"].as_f64().unwrap_or(0.0)))
        .replace("{top}", &js_number(box_["top"].as_f64().unwrap_or(0.0)))
        .replace("{language}", languages.first().map_or("", String::as_str))
        .replace("{languages}", &languages.join(","))
}

pub(super) fn run_provider(
    config: &ProviderConfig,
    png: &[u8],
    source: &str,
    region: &Value,
    languages: &[String],
    timeout_ms: u64,
    max_output_chars: usize,
) -> Result<String, CommandRunError> {
    match config {
        ProviderConfig::Http { .. } => {
            http::run_http_provider(config, png, source, region, languages, timeout_ms)
        }
        ProviderConfig::Command {
            command,
            args_template,
        } => {
            let page = region["page"].as_u64().unwrap_or(0);
            let region_id = region["region_id"].as_str().unwrap_or("unknown");
            let temp = TempDir::with_prefix("pdf-reader-mcp-region-analysis-").map_err(|_| {
                CommandRunError::new(
                    "Failed to create region analysis provider workspace.".into(),
                    0,
                )
            })?;
            let input = temp.path().join(format!("region-{page}.png"));
            std::fs::write(&input, png).map_err(|_| {
                CommandRunError::new(
                    "Failed to write region analysis provider input image.".into(),
                    0,
                )
            })?;
            let input = input.to_string_lossy();
            let args = args_template
                .iter()
                .map(|arg| replace_placeholders(arg, &input, source, region, languages))
                .collect();
            command_provider::run(CommandInvocation {
                command: command.clone(),
                args,
                timeout_ms,
                max_stdout_bytes: max_output_chars.saturating_mul(4).max(1024 * 1024),
                failure_message: format!(
                    "Region analysis provider command failed for page {page} region {region_id}."
                ),
                timeout_message: format!(
                    "Region analysis provider command timed out for page {page} region {region_id}."
                ),
            })
        }
    }
}
