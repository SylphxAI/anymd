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

async fn run_async(invocation: CommandInvocation) -> Result<String, CommandRunError> {
    let mut command = Command::new(&invocation.command);
    command
        .args(&invocation.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .group_spawn()
        .map_err(|_| CommandRunError::new(invocation.failure_message.clone(), 0))?;
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
            .block_on(run_async(invocation))
    })
    .join()
    .map_err(|_| CommandRunError::new("Command provider worker failed.".into(), 0))?
}
