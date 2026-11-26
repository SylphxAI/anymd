//! Markdown transcript output for legacy conversion.

use super::TranscriptCue;

pub(in crate::video) fn render_transcript(cues: &[TranscriptCue]) -> String {
    if cues.is_empty() {
        return String::new();
    }
    let kind = if cues.iter().all(|c| c.word) {
        "word (Qwen3-ForcedAligner)"
    } else if cues.iter().any(|c| c.word) {
        "mixed word and segment"
    } else {
        "segment (20-second chunk boundaries, not word timing)"
    };
    let clock = |ms: u64| {
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            ms / 3_600_000,
            ms / 60_000 % 60,
            ms / 1000 % 60,
            ms % 1000
        )
    };
    let lines = cues
        .iter()
        .map(|c| format!("[{} --> {}] {}", clock(c.start_ms), clock(c.end_ms), c.text))
        .collect::<Vec<_>>()
        .join("\n");
    format!("_Timestamps: {kind}._\n\n{lines}")
}
