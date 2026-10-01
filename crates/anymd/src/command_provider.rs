//! Shared bounded execution for opt-in local command providers.

use std::process::Stdio;
use std::thread;
use std::time::Duration;

use command_group::AsyncCommandGroup;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

#[derive(Clone)]
pub struct CommandInvocation {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub failure_message: String,
    pub timeout_message: String,
}

#[derive(Debug)]
pub struct CommandRunError {
    pub message: String,
    /// Provider stdout bytes to charge to the request aggregate. When a timed
    /// out/read-failed invocation cannot report the exact count, this is the
    /// per-call maximum so failure paths cannot bypass aggregate admission.
    pub charge_bytes: usize,
}

impl CommandRunError {
    pub fn new(message: String, charge_bytes: usize) -> Self {
        Self {
            message,
            charge_bytes,
        }
    }
}

async fn read_bounded<R: AsyncRead + Unpin>(reader: R, maximum: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

async fn run_async(
    invocation: CommandInvocation,
    supervised: bool,
) -> Result<String, CommandRunError> {
    let mut command = Command::new(&invocation.command);
    command
        .args(&invocation.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if supervised {
        // Only cooperating native workers use stdin as a parent-held liveness
        // pipe. EOF survives abrupt parent death even across process groups.
        command
            .arg(format!("--supervised={}", invocation.timeout_ms))
            .stdin(Stdio::piped());
    }
    let mut child = command
        .group_spawn()
        .map_err(|_| CommandRunError::new(invocation.failure_message.clone(), 0))?;
    // Keep this writer alive for the entire invocation; never pass it to another
    // process. Direct benchmark workers retain their ordinary null stdin.
    let _liveness = child.inner().stdin.take();
    let stdout = child
        .inner()
        .stdout
        .take()
        .ok_or_else(|| CommandRunError::new(invocation.failure_message.clone(), 0))?;
    let stderr = child
        .inner()
        .stderr
        .take()
        .ok_or_else(|| CommandRunError::new(invocation.failure_message.clone(), 0))?;
    let execution = async {
        tokio::join!(
            child.inner().wait(),
            read_bounded(stdout, invocation.max_stdout_bytes),
            read_bounded(stderr, 64 * 1024)
        )
    };
    let (status, stdout, stderr) =
        match tokio::time::timeout(Duration::from_millis(invocation.timeout_ms), execution).await {
            Ok(results) => results,
            Err(_) => {
                // Dropping the timed-out read futures closes their pipe handles. Tree
                // termination and leader reaping stay best-effort and bounded so an
                // escaped descendant cannot extend the request deadline.
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(2), child.inner().wait()).await;
                return Err(CommandRunError::new(
                    invocation.timeout_message,
                    invocation.max_stdout_bytes,
                ));
            }
        };
    // Terminate any helper left in the process group/job after the leader exits.
    let _ = child.start_kill();
    let status = status.map_err(|_| {
        CommandRunError::new(
            invocation.failure_message.clone(),
            invocation.max_stdout_bytes,
        )
    })?;
    let stdout = stdout.map_err(|_| {
        CommandRunError::new(
            invocation.failure_message.clone(),
            invocation.max_stdout_bytes,
        )
    })?;
    let _stderr = stderr
        .map_err(|_| CommandRunError::new(invocation.failure_message.clone(), stdout.len()))?;
    if !status.success() || stdout.len() > invocation.max_stdout_bytes {
        return Err(CommandRunError::new(
            invocation.failure_message,
            stdout.len(),
        ));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

pub fn run(invocation: CommandInvocation) -> Result<String, CommandRunError> {
    run_with_supervision(invocation, false)
}

pub fn run_supervised(invocation: CommandInvocation) -> Result<String, CommandRunError> {
    run_with_supervision(invocation, true)
}

/// A cooperating worker must install this before loading models or starting
/// work. EOF/error means its caller is gone; the independent deadline also
/// bounds a worker whose parent is alive but no longer driving the request.
pub fn supervise_parent(timeout_ms: u64) -> Result<(), String> {
    if timeout_ms == 0 || timeout_ms > 600_000 {
        return Err("Invalid supervised worker deadline".into());
    }
    thread::Builder::new()
        .name("worker-parent-liveness".into())
        .spawn(|| {
            use std::io::Read;
            let mut byte = [0];
            loop {
                match std::io::stdin().read(&mut byte) {
                    Ok(0) | Err(_) => std::process::exit(124),
                    Ok(_) => {}
                }
            }
        })
        .map_err(|e| e.to_string())?;
    thread::Builder::new()
        .name("worker-deadline".into())
        .spawn(move || {
            thread::sleep(Duration::from_millis(timeout_ms));
            std::process::exit(124);
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn run_with_supervision(
    invocation: CommandInvocation,
    supervised: bool,
) -> Result<String, CommandRunError> {
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| {
                CommandRunError::new(
                    "Failed to start bounded command provider runtime.".into(),
                    0,
                )
            })?
            .block_on(run_async(invocation, supervised))
    })
    .join()
    .map_err(|_| CommandRunError::new("Command provider worker failed.".into(), 0))?
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn deadline_terminates_the_worker_instead_of_detaching_it() {
        let started = std::time::Instant::now();
        let result = run(CommandInvocation {
            command: "sh".into(),
            args: vec!["-c".into(), "sleep 30".into()],
            timeout_ms: 100,
            max_stdout_bytes: 1024,
            failure_message: "worker failed".into(),
            timeout_message: "page timeout".into(),
        });
        assert_eq!(result.unwrap_err().message, "page timeout");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
