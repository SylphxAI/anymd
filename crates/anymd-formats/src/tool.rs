//! Local helper binaries (ffprobe, ffmpeg, tesseract, optional forced aligner): found on
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
    run_captured(program, args, env, timeout, MAX_OUTPUT, MAX_OUTPUT, false)
}

/// Same execution owner with lower operation-specific caps. A truncated result
/// is an error, never usable evidence. No separate child runner is introduced.
pub(crate) fn run_bounded<I, S>(
    program: &Path,
    args: I,
    timeout: Duration,
    stdout_limit: u64,
    stderr_limit: u64,
) -> Result<ToolOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    if timeout.is_zero() || stdout_limit > MAX_OUTPUT || stderr_limit > MAX_OUTPUT {
        return Err("expired deadline or invalid local tool output budget".into());
    }
    run_captured(
        program,
        args,
        &[],
        timeout,
        stdout_limit,
        stderr_limit,
        true,
    )
}

fn run_captured<I, S>(
    program: &Path,
    args: I,
    env: &[(&str, &str)],
    timeout: Duration,
    stdout_limit: u64,
    stderr_limit: u64,
    reject_truncation: bool,
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
    let stdout = child.stdout.take().map(|r| drain(r, stdout_limit));
    let stderr = child.stderr.take().map(|r| drain(r, stderr_limit));
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
    let collect = |handle: Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>| {
        handle
            .ok_or_else(|| format!("{name} missing captured pipe"))?
            .join()
            .map_err(|_| format!("{name} output reader failed"))?
            .map_err(|e| format!("{name} output read failed: {e}"))
    };
    let stdout = collect(stdout);
    let stderr = collect(stderr);
    let mut stdout = if reject_truncation {
        stdout?
    } else {
        stdout.unwrap_or_default()
    };
    let mut stderr = if reject_truncation {
        stderr?
    } else {
        stderr.unwrap_or_default()
    };
    if reject_truncation
        && (stdout.len() as u64 > stdout_limit || stderr.len() as u64 > stderr_limit)
    {
        return Err(format!("{name} exceeded the operation output budget"));
    }
    stdout.truncate(stdout_limit as usize);
    stderr.truncate(stderr_limit as usize);
    Ok(ToolOutput {
        success: status.success(),
        stdout,
        stderr,
    })
}

fn drain<R: Read + Send + 'static>(
    reader: R,
    maximum: u64,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut limited = reader.take(maximum.saturating_add(1));
        limited.read_to_end(&mut buf)?;
        // Keep draining past the cap so the child never blocks on a full pipe.
        std::io::copy(&mut limited.into_inner(), &mut std::io::sink())?;
        Ok(buf)
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
    fn bounded_capture_retains_overflow_marker_and_rejects_expired_before_spawn() {
        let bytes = drain(std::io::Cursor::new(vec![1u8; 10]), 4)
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(bytes.len(), 5);
        let error = run_bounded(Path::new("not-executed"), ["unused"], Duration::ZERO, 4, 4)
            .err()
            .unwrap();
        assert!(error.contains("expired"));
    }

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
