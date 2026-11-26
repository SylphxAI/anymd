//! ForcedAligner invocation and word alignment validation.

#[cfg(feature = "native")]
use std::{path::Path, time::Duration};

use serde_json::Value;

use super::TranscriptCue;
#[cfg(feature = "native")]
use crate::tool;

#[cfg(feature = "native")]
pub(super) fn align_words(
    binary: &Path,
    model: &Path,
    wav: &Path,
    text: &str,
    duration_ms: u64,
    timeout: Duration,
    output_limit: Option<u64>,
) -> Result<Vec<TranscriptCue>, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let reference = dir.path().join("reference.txt");
    std::fs::write(&reference, text).map_err(|e| e.to_string())?;
    let args = [
        std::ffi::OsStr::new("--align-only"),
        "-am".as_ref(),
        model.as_os_str(),
        "-f".as_ref(),
        wav.as_os_str(),
        "--text-file".as_ref(),
        reference.as_os_str(),
        "--align-format".as_ref(),
        "json".as_ref(),
        "--align-granularity".as_ref(),
        "word".as_ref(),
        "-t".as_ref(),
        "4".as_ref(),
        "-ng".as_ref(),
    ];
    let output = match output_limit {
        Some(limit) => tool::run_bounded(binary, args, timeout, limit, 64 * 1024)?,
        None => tool::run(binary, args, timeout)?,
    };

    if !output.success {
        return Err("ForcedAligner failed; retaining segment timestamps".into());
    }
    let payload = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    super::parse_alignment(&payload, text, duration_ms)
}

pub(in crate::video) fn parse_alignment(
    payload: &str,
    text: &str,
    duration_ms: u64,
) -> Result<Vec<TranscriptCue>, String> {
    let root: Value =
        serde_json::from_str(payload).map_err(|e| format!("ForcedAligner JSON: {e}"))?;
    let rows = root
        .as_array()
        .ok_or("ForcedAligner JSON must be a word array")?;
    let mut cues = Vec::new();
    let mut previous_end = 0u64;
    for row in rows {
        let start = row
            .get("start")
            .and_then(Value::as_f64)
            .ok_or("missing word start")?;
        let end = row
            .get("end")
            .and_then(Value::as_f64)
            .ok_or("missing word end")?;
        let word = row
            .get("word")
            .and_then(Value::as_str)
            .ok_or("missing alignment word")?;
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || end < start
            || end * 1000.0 > duration_ms as f64
        {
            return Err("word timestamps outside audio".into());
        }
        let start_ms = (start * 1000.0).round() as u64;
        let end_ms = (end * 1000.0).round() as u64;
        if start_ms < previous_end || word.trim().is_empty() {
            return Err("invalid word alignment".into());
        }
        previous_end = end_ms;
        cues.push(TranscriptCue {
            start_ms,
            end_ms,
            text: word.into(),
            word: true,
        });
    }
    let letters = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let aligned = cues.iter().map(|c| c.text.as_str()).collect::<String>();
    if cues.is_empty() || letters(&aligned) != letters(text) {
        return Err("word alignment omitted or changed transcript text".into());
    }
    Ok(cues)
}
