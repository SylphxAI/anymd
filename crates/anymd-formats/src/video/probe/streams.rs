//! Stream classification and Markdown descriptions.

use serde_json::Value;

use super::media::{number, tag_value};

pub(super) fn kind_of(stream: &Value) -> &str {
    stream
        .get("codec_type")
        .and_then(Value::as_str)
        .unwrap_or("")
}

pub(super) fn is_cover(stream: &Value) -> bool {
    stream
        .get("disposition")
        .and_then(|d| d.get("attached_pic"))
        .and_then(Value::as_i64)
        == Some(1)
}

pub(super) fn append_streams(markdown: &mut String, streams: &[Value]) {
    let stream_lines: Vec<String> = streams
        .iter()
        .filter_map(|s| describe_stream(s, is_cover(s)))
        .collect();
    if !stream_lines.is_empty() {
        markdown.push_str("\n\n## Streams\n\n");
        markdown.push_str(&stream_lines.join("\n"));
    }
}

fn describe_stream(stream: &Value, cover: bool) -> Option<String> {
    let kind = stream.get("codec_type").and_then(Value::as_str)?;
    let codec = stream
        .get("codec_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut parts = vec![codec.to_string()];
    match kind {
        "video" => {
            if let (Some(w), Some(h)) = (
                stream.get("width").and_then(Value::as_u64),
                stream.get("height").and_then(Value::as_u64),
            ) {
                parts.push(format!("{w}×{h}"));
            }
            if !cover {
                let fps = ["avg_frame_rate", "r_frame_rate"]
                    .iter()
                    .filter_map(|k| stream.get(*k).and_then(Value::as_str).and_then(frame_rate))
                    .next();
                if let Some(fps) = fps {
                    parts.push(format!("{} fps", trim_float(fps)));
                }
            }
        }
        "audio" => {
            if let Some(rate) = number(stream.get("sample_rate")) {
                parts.push(format!("{} kHz", trim_float(rate / 1000.0)));
            }
            let layout = stream.get("channel_layout").and_then(Value::as_str);
            match (stream.get("channels").and_then(Value::as_u64), layout) {
                (Some(ch), Some(layout)) => parts.push(format!("{layout} ({ch} ch)")),
                (Some(ch), None) => parts.push(format!("{ch} ch")),
                _ => {}
            }
        }
        _ => {}
    }
    if let Some(lang) = tag_value(stream.get("tags"), "language").filter(|l| l != "und") {
        parts.push(format!("lang {lang}"));
    }
    if let Some(title) = tag_value(stream.get("tags"), "title") {
        parts.push(format!("\"{title}\""));
    }
    let label = match kind {
        "video" if cover => "Cover art",
        "video" => "Video",
        "audio" => "Audio",
        "subtitle" => "Subtitle",
        "data" => return None,
        "attachment" => "Attachment",
        _ => "Stream",
    };
    Some(format!("- {label}: {}", parts.join(", ")))
}

fn frame_rate(value: &str) -> Option<f64> {
    let (num, den) = value.split_once('/')?;
    let (num, den): (f64, f64) = (num.parse().ok()?, den.parse().ok()?);
    (den > 0.0 && num > 0.0).then(|| num / den)
}

fn trim_float(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}
