//! Supplemental subtitle and transcript sections, with their media notes.

use std::path::Path;

use serde_json::Value;

use super::super::subtitles::{extract_subtitles, sidecar_sections};
use super::super::transcribe::transcribe;
use super::streams::kind_of;
use crate::{Options, Section};

pub(super) fn sections(
    streams: &[Value],
    path: &Path,
    given_path: Option<&Path>,
    options: &Options,
    source_access: &dyn crate::SourceAccess,
) -> (Vec<Section>, Vec<String>) {
    let mut extra_sections = Vec::new();
    let mut notes = Vec::new();

    // Try the first embedded subtitle stream; image subtitles need OCR.
    if let Some(first) = streams.iter().find(|s| kind_of(s) == "subtitle") {
        let codec = first
            .get("codec_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if matches!(
            codec,
            "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub"
        ) {
            notes.push(format!(
                "_Embedded subtitles are images ({codec}); they need OCR and are not extracted._"
            ));
        } else {
            match extract_subtitles(path) {
                Ok(text) if !text.is_empty() => extra_sections.push(Section {
                    label: "subtitles".into(),
                    markdown: text,
                }),
                Ok(_) => {}
                Err(error) => {
                    notes.push(format!("_Embedded subtitles were not extracted: {error}_"))
                }
            }
        }
    }
    extra_sections.extend(sidecar_sections(given_path, source_access));

    if streams.iter().any(|s| kind_of(s) == "audio") {
        if options.transcript {
            match transcribe(path, options.download_asr_model) {
                Ok(text) if !text.is_empty() => extra_sections.push(Section {
                    label: "transcript".into(),
                    markdown: text,
                }),
                Ok(_) => notes.push("_Transcript: no speech was recognised._".into()),
                Err(error) => {
                    // First line is the summary; any further lines are how-to bullets.
                    let (head, rest) = error.split_once('\n').unwrap_or((&error, ""));
                    let mut note = format!("_Transcript unavailable: {head}_");
                    if !rest.is_empty() {
                        note.push('\n');
                        note.push_str(rest);
                    }
                    notes.push(note);
                }
            }
        } else {
            notes.push(
                "_Pass `transcript: true` for a speech transcript (local Qwen3-ASR; `anymd doctor` shows what is installed)._"
                    .into(),
            );
        }
    }
    (extra_sections, notes)
}
