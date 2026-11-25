//! HTTP region-analysis transport and preset request/response formats.

use std::time::Duration;

use serde_json::{json, Value};

use crate::command_provider::CommandRunError;

use super::{js_number, ProviderConfig};

fn vision_prompt(region: &Value, source: &str, languages: &[String]) -> String {
    let box_ = &region["source_bounding_box"];
    format!(
        "Analyze this cropped PDF region for an AI document parser.\nSource: {source}\nRegion id: {}\nPage: {}\nPDF bounding box: left={}, bottom={}, right={}, top={}\nLanguages: {}\nReturn only one JSON object with kind/description/table/formula/chart fields when applicable.",
        region["region_id"].as_str().unwrap_or(""),
        region["page"].as_u64().unwrap_or(0),
        js_number(box_["left"].as_f64().unwrap_or(0.0)),
        js_number(box_["bottom"].as_f64().unwrap_or(0.0)),
        js_number(box_["right"].as_f64().unwrap_or(0.0)),
        js_number(box_["top"].as_f64().unwrap_or(0.0)),
        if languages.is_empty() {
            "unspecified".into()
        } else {
            languages.join(",")
        }
    )
}

fn build_http_request_body(
    config: &ProviderConfig,
    image_b64: &str,
    mime_type: &str,
    source: &str,
    region: &Value,
    languages: &[String],
) -> Result<Value, String> {
    let ProviderConfig::Http { preset, model, .. } = config else {
        return Err("Internal error: HTTP body builder called for non-HTTP provider.".into());
    };
    match preset.as_deref() {
        Some("ollama") => Ok(json!({
            "model": model,
            "prompt": vision_prompt(region, source, languages),
            "images": [image_b64],
            "stream": false,
            "format": "json",
        })),
        Some("openai-compatible" | "lmstudio" | "llamacpp") => Ok(json!({
            "model": model,
            "messages": [
                {
                    "role": "system",
                    "content": "You analyze cropped PDF regions for an AI document parser. Return only one JSON object."
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": vision_prompt(region, source, languages)},
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{mime_type};base64,{image_b64}")
                            }
                        }
                    ]
                }
            ],
            "temperature": 0
        })),
        _ => Ok(json!({
            "image_base64": image_b64,
            "mime_type": mime_type,
            "format": region.get("format").cloned().unwrap_or_else(|| json!("png")),
            "page": region.get("page").cloned().unwrap_or(json!(0)),
            "region_id": region.get("region_id").cloned().unwrap_or(json!("")),
            "evidence_id": region.get("evidence_id").cloned().unwrap_or(json!("")),
            "source": source,
            "source_bounding_box": region.get("source_bounding_box").cloned().unwrap_or(json!({})),
            "crop_pixels": region.get("crop_pixels").cloned().unwrap_or(json!({})),
            "scale": region.get("scale").cloned().unwrap_or(json!(1.0)),
            "languages": languages,
        })),
    }
}

fn parse_http_provider_stdout(preset: Option<&str>, stdout: &str) -> Result<String, String> {
    match preset {
        Some("ollama") => {
            let parsed: Value = serde_json::from_str(stdout).map_err(|_| {
                "Ollama region analysis response was not a JSON object.".to_string()
            })?;
            let response = parsed
                .get("response")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "Ollama region analysis response did not include a non-empty response string."
                        .to_string()
                })?;
            Ok(response.to_string())
        }
        Some("openai-compatible" | "lmstudio" | "llamacpp") => {
            let parsed: Value = serde_json::from_str(stdout).map_err(|_| {
                "OpenAI-compatible region analysis response was not a JSON object.".to_string()
            })?;
            let content = parsed
                .pointer("/choices/0/message/content")
                .cloned()
                .ok_or_else(|| {
                    "OpenAI-compatible region analysis response did not include message content."
                        .to_string()
                })?;
            if let Some(text) = content
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Ok(text.to_string());
            }
            if let Some(parts) = content.as_array() {
                let text = parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return Ok(text);
                }
            }
            Err(
                "OpenAI-compatible region analysis response did not include message content."
                    .into(),
            )
        }
        _ => Ok(stdout.to_string()),
    }
}

pub(super) fn run_http_provider(
    config: &ProviderConfig,
    png: &[u8],
    source: &str,
    region: &Value,
    languages: &[String],
    timeout_ms: u64,
) -> Result<String, CommandRunError> {
    let ProviderConfig::Http {
        url,
        headers,
        preset,
        ..
    } = config
    else {
        return Err(CommandRunError::new(
            "Internal error: HTTP provider invoked for non-HTTP config.".into(),
            0,
        ));
    };
    let page = region["page"].as_u64().unwrap_or(0);
    let region_id = region["region_id"].as_str().unwrap_or("unknown");
    let mime_type = region["mime_type"].as_str().unwrap_or("image/png");
    let image_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png);
    let body =
        match build_http_request_body(config, &image_b64, mime_type, source, region, languages) {
            Ok(body) => body,
            Err(message) => return Err(CommandRunError::new(message, 0)),
        };
    // Statuses come back as responses, not errors, so a failed call keeps its
    // body for the byte count below.
    let agent: ureq::Agent = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_millis(timeout_ms.max(1))))
        .max_redirects(5)
        // No proxy, as on 2.x: `proxy-from-env` is not a default feature there,
        // while 3.x's `Config` default does read the environment.
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into();
    let mut request = agent.post(url).header("Content-Type", "application/json");
    for (key, value) in headers {
        request = request.header(key, value);
    }
    let response = match request.send_json(body) {
        Ok(response) => response,
        Err(error) => {
            let timed_out = matches!(&error, ureq::Error::Timeout(_))
                || error.to_string().to_ascii_lowercase().contains("timed out");
            return Err(CommandRunError::new(
                if timed_out {
                    format!(
                        "Region analysis HTTP provider timed out for page {page} region {region_id}."
                    )
                } else {
                    format!(
                        "Region analysis HTTP provider failed for page {page} region {region_id}."
                    )
                },
                0,
            ));
        }
    };
    let status = response.status().as_u16();
    let stdout = response.into_body().read_to_string().map_err(|_| {
        CommandRunError::new(
            format!("Region analysis HTTP provider failed for page {page} region {region_id}."),
            0,
        )
    })?;
    if !(200..300).contains(&status) {
        return Err(CommandRunError::new(
            format!("Region analysis HTTP provider failed with status {status}."),
            stdout.len(),
        ));
    }
    parse_http_provider_stdout(preset.as_deref(), &stdout)
        .map_err(|message| CommandRunError::new(message, stdout.len()))
}
