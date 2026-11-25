//! Run ffprobe and validate its response before rendering.

use std::path::Path;

use serde_json::Value;

use super::super::PROBE_TIMEOUT;
use crate::tool;

pub(super) fn read(ffprobe: &Path, path: &Path) -> Result<Value, String> {
    let args: [&std::ffi::OsStr; 9] = [
        "-v".as_ref(),
        "quiet".as_ref(),
        "-print_format".as_ref(),
        "json".as_ref(),
        "-show_format".as_ref(),
        "-show_streams".as_ref(),
        "-show_chapters".as_ref(),
        "--".as_ref(),
        path.as_os_str(),
    ];
    let output = tool::run(ffprobe, args, PROBE_TIMEOUT)?;
    if !output.success {
        return Err("ffprobe exited with an error".into());
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("ffprobe returned unreadable JSON: {e}"))?;
    let has_streams = value
        .get("streams")
        .and_then(Value::as_array)
        .is_some_and(|s| !s.is_empty());
    if has_streams {
        Ok(value)
    } else {
        Err("no audio or video streams found".into())
    }
}
