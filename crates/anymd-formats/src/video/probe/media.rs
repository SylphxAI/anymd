//! Container metadata, facts table, and chapter rendering.

use serde_json::Value;

pub(super) fn format_facts(
    format: &Value,
    sniffed: Option<&str>,
    has_video: bool,
) -> Vec<(String, String)> {
    let tag = |key: &str| tag_value(format.get("tags"), key);
    let mut facts: Vec<(String, String)> = Vec::new();
    let container_name = format
        .get("format_long_name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| sniffed.map(str::to_string))
        .unwrap_or_else(|| "unknown".into());
    facts.push(("container".into(), container_name));
    facts.push((
        "kind".into(),
        if has_video { "video" } else { "audio" }.into(),
    ));
    if let Some(duration) = number(format.get("duration")) {
        facts.push(("duration".into(), timestamp(duration, duration >= 3600.0)));
    }
    if let Some(size) = number(format.get("size")) {
        facts.push(("file size".into(), crate::image::human_bytes(size as u64)));
    }
    if let Some(rate) = number(format.get("bit_rate")) {
        facts.push(("bitrate".into(), format!("{:.0} kb/s", rate / 1000.0)));
    }
    for key in ["artist", "album", "date", "genre", "comment"] {
        if let Some(value) = tag(key) {
            facts.push((key.into(), value.chars().take(200).collect()));
        }
    }
    facts
}

pub(super) fn append_chapters(markdown: &mut String, probe: &Value, format: &Value) {
    let chapters = probe
        .get("chapters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let long = number(format.get("duration")).is_some_and(|d| d >= 3600.0);
    let chapter_lines: Vec<String> = chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            let start = number(chapter.get("start_time")).unwrap_or(0.0);
            let title = tag_value(chapter.get("tags"), "title")
                .unwrap_or_else(|| format!("Chapter {}", index + 1));
            format!("- {} {}", timestamp(start, long), title)
        })
        .collect();
    if !chapter_lines.is_empty() {
        markdown.push_str("\n\n## Chapters\n\n");
        markdown.push_str(&chapter_lines.join("\n"));
    }
}

pub(super) fn facts_table(facts: &[(String, String)]) -> String {
    let mut rows = vec![vec!["Property".to_string(), "Value".to_string()]];
    for (key, value) in facts {
        let mut label = key.clone();
        if let Some(first) = label.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        rows.push(vec![label, value.clone()]);
    }
    crate::markdown_table(&rows).trim_end().to_string()
}

pub(super) fn tag_value(tags: Option<&Value>, key: &str) -> Option<String> {
    let tags = tags?.as_object()?;
    tags.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, v)| v.as_str())
        .map(|v| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|v| !v.is_empty())
}

pub(super) fn number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let n = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())?;
    n.is_finite().then_some(n)
}

fn timestamp(seconds: f64, hours: bool) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours || h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}
