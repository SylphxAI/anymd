//! Without `native` no helper binary exists: every lookup misses,
//! so OCR, ffprobe, and ASR paths fall back to their no-tool output.

use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) struct ToolOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(crate) struct TempFile(PathBuf);

impl TempFile {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

pub(crate) fn find(_name: &str) -> Option<PathBuf> {
    None
}

pub(crate) fn run<I, S>(_program: &Path, _args: I, _timeout: Duration) -> Result<ToolOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Err("local tools are not available in this build".into())
}

pub(crate) fn run_with_env<I, S>(
    program: &Path,
    args: I,
    _env: &[(&str, &str)],
    timeout: Duration,
) -> Result<ToolOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    run(program, args, timeout)
}

pub(crate) fn temp_file(_bytes: &[u8], _suffix: &str) -> Result<TempFile, String> {
    Err("temp files are not available in this build".into())
}
