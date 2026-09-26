//! Local helper binaries (ffprobe, ffmpeg, tesseract, whisper.cpp): found on
//! PATH, run without a shell, bounded by a timeout and an output cap.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

/// Cap on captured stdout/stderr so a runaway tool cannot exhaust memory.
const MAX_OUTPUT: u64 = 64 * 1024 * 1024;

pub(crate) struct ToolOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Locate an executable on PATH.
pub(crate) fn find(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        if cfg!(windows) {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
        None
    })
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Run `program args...` with a timeout; the child is killed when it expires.
pub(crate) fn run<I, S>(program: &Path, args: I, timeout: Duration) -> Result<ToolOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_with_env(program, args, &[], timeout)
}

/// [`run`] with extra environment variables for the child.
pub(crate) fn run_with_env<I, S>(
    program: &Path,
    args: I,
    env: &[(&str, &str)],
    timeout: Duration,
) -> Result<ToolOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut child = Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {name}: {e}"))?;
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{name} timed out after {}s", timeout.as_secs()));
        }
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{name} failed: {e}"));
        }
    };
    let collect = |handle: Option<std::thread::JoinHandle<Vec<u8>>>| {
        handle.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    Ok(ToolOutput {
        success: status.success(),
        stdout: collect(stdout),
        stderr: collect(stderr),
    })
}

fn drain<R: Read + Send + 'static>(reader: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut limited = reader.take(MAX_OUTPUT);
        let _ = limited.read_to_end(&mut buf);
        // Keep draining past the cap so the child never blocks on a full pipe.
        let _ = std::io::copy(&mut limited.into_inner(), &mut std::io::sink());
        buf
    })
}

/// Bytes on disk for tools that need a path; removed when dropped.
pub(crate) fn temp_file(bytes: &[u8], suffix: &str) -> Result<tempfile::NamedTempFile, String> {
    let mut file = tempfile::Builder::new()
        .prefix("anymd-")
        .suffix(suffix)
        .tempfile()
        .map_err(|e| format!("could not create a temp file: {e}"))?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .map_err(|e| format!("could not write a temp file: {e}"))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tool_is_none_and_timeouts_kill() {
        assert!(find("anymd-definitely-not-a-binary").is_none());
        if let Some(sleep) = find("sleep") {
            let err = run(&sleep, ["5"], Duration::from_millis(200))
                .err()
                .unwrap();
            assert!(err.contains("timed out"), "{err}");
        }
    }
}
