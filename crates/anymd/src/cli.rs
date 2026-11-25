//! Command-line mode: `anymd <file|url|dir>...` prints Markdown to stdout,
//! `anymd search <query> [paths...]` prints hits. With no arguments and a
//! piped stdin the binary runs the MCP server instead (see main.rs).

use std::io::IsTerminal;

use crate::document::EvidenceKind;
use crate::source_access::SourceAccessPolicy;

mod args;
mod cite_check;
mod convert;
mod discovery;
mod dispatch;
mod output;

#[cfg(test)]
mod tests;

pub use output::USAGE;

/// What main should do with the process arguments.
pub enum Mode {
    Mcp,
    Doctor,
    Version,
    Setup(Vec<String>),
    Pro(Vec<String>),
    Login(Vec<String>),
    Cli(Vec<String>),
}

/// Decide between MCP server and CLI. MCP configs pass no file arguments
/// (optionally `--allow-dir=...`) and pipe stdin.
pub fn mode(arguments: &[String]) -> Mode {
    match arguments.first().map(String::as_str) {
        Some("mcp") | Some("serve") => return Mode::Mcp,
        Some("doctor") => return Mode::Doctor,
        Some("version") => return Mode::Version,
        Some("setup") => return Mode::Setup(arguments[1..].to_vec()),
        Some("pro") => return Mode::Pro(arguments[1..].to_vec()),
        Some("login") => return Mode::Login(arguments[1..].to_vec()),
        _ => {}
    }
    let only_server_flags = arguments
        .iter()
        .all(|argument| argument.starts_with("--allow-dir"));
    if only_server_flags && (!arguments.is_empty() || !std::io::stdin().is_terminal()) {
        return Mode::Mcp;
    }
    Mode::Cli(arguments.to_vec())
}

/// Run the CLI; returns the process exit code.
pub fn run(arguments: Vec<String>, policy: &SourceAccessPolicy) -> i32 {
    run_with_evidence(arguments, policy, |_| {})
}

/// Notify the CLI caller only after qualifying work and output both succeed.
pub fn run_with_evidence(
    arguments: Vec<String>,
    policy: &SourceAccessPolicy,
    on_evidence: impl FnOnce(EvidenceKind),
) -> i32 {
    dispatch::run(&arguments, policy, on_evidence)
}
