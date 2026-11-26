use super::resolve::models_dir;
use super::{ASR, MODEL_ENV};
use std::path::PathBuf;

pub fn ffmpeg_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "`brew install ffmpeg`"
    } else if cfg!(windows) {
        "`winget install Gyan.FFmpeg`"
    } else {
        "your package manager, e.g. `sudo apt install ffmpeg`"
    }
}

pub fn status_lines() -> Vec<(String, String)> {
    let model = std::env::var_os(MODEL_ENV)
        .map(PathBuf::from)
        .or_else(|| models_dir().map(|d| d.join(ASR.name)));
    let state = match model {
        Some(path) if path.is_file() => {
            format!("found {} (verified before inference)", path.display())
        }
        _ => format!(
            "not installed: {} ({}, SHA-256 pinned)",
            ASR.name,
            ASR.size_label()
        ),
    };
    vec![
        (
            "ASR runtime".into(),
            "transcribe-cpp 0.2.4 (CPU, bundled)".into(),
        ),
        ("ASR model".into(), state),
    ]
}
