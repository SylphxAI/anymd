//! Probe media and assemble its Markdown conversion.

mod ffprobe;
mod media;
mod streams;
mod supplemental;

use std::path::Path;

use self::media::{append_chapters, facts_table, format_facts, tag_value};
use self::streams::{append_streams, is_cover, kind_of};
use super::subtitles::sidecar_sections;
use crate::{Converted, Options, Section};
use serde_json::Value;

pub(super) fn probe(ffprobe: &Path, path: &Path) -> Result<Value, String> {
    ffprobe::read(ffprobe, path)
}

pub(super) fn basic(
    container: &str,
    size: u64,
    path: Option<&Path>,
    source_access: &dyn crate::SourceAccess,
    note: &str,
) -> Converted {
    let facts = vec![
        ("container".to_string(), container.to_string()),
        ("file size".to_string(), crate::image::human_bytes(size)),
    ];
    let mut markdown = facts_table(&facts);
    markdown.push_str("\n\n");
    markdown.push_str(note);
    let mut sections = vec![Section {
        label: "media".into(),
        markdown,
    }];
    sections.extend(sidecar_sections(path, source_access));
    Converted {
        outline: Vec::new(),
        format: "video".into(),
        title: None,
        sections,
        metadata: facts,
    }
}

pub(super) fn render_probe(
    probe: &Value,
    path: &Path,
    given_path: Option<&Path>,
    sniffed: Option<&str>,
    options: &Options,
    source_access: &dyn crate::SourceAccess,
) -> Converted {
    let format = probe.get("format").unwrap_or(&Value::Null);
    let streams = probe
        .get("streams")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let has_video = streams
        .iter()
        .any(|s| kind_of(s) == "video" && !is_cover(s));

    let facts = format_facts(format, sniffed, has_video);
    let mut markdown = facts_table(&facts);
    append_streams(&mut markdown, streams);
    append_chapters(&mut markdown, probe, format);

    let (extra_sections, notes) =
        supplemental::sections(streams, path, given_path, options, source_access);
    if !notes.is_empty() {
        markdown.push_str("\n\n");
        markdown.push_str(&notes.join("\n"));
    }

    let mut sections = vec![Section {
        label: "media".into(),
        markdown,
    }];
    sections.extend(extra_sections);
    Converted {
        outline: Vec::new(),
        format: "video".into(),
        title: tag_value(format.get("tags"), "title"),
        sections,
        metadata: facts,
    }
}
