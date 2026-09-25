//! `anymd setup`: register anymd with the MCP clients on this machine.
//! mcp-kit does the client detection and config editing.

use mcp_kit::setup::{self, Options, Server};

/// Run `anymd setup [--dry-run] [--remove] [--client=a,b]`; returns the exit code.
pub fn run(arguments: &[String]) -> i32 {
    let mut options = Options::default();
    let mut iter = arguments.iter();
    while let Some(argument) = iter.next() {
        match argument.as_str() {
            "--dry-run" => options.dry_run = true,
            "--remove" => options.remove = true,
            "--client" => match iter.next() {
                Some(list) => options.clients = Some(split_clients(list)),
                None => return usage_error("--client needs a value"),
            },
            other => match other.strip_prefix("--client=") {
                Some(list) => options.clients = Some(split_clients(list)),
                None => return usage_error(&format!("unknown option {other}")),
            },
        }
    }
    println!(
        "anymd setup{}",
        if options.dry_run { " (dry run)" } else { "" }
    );
    let server = Server {
        name: "anymd".into(),
        package: "@sylphx/anymd".into(),
        args: vec!["mcp".into()],
    };
    match setup::run(&server, &options) {
        Ok(touched) => {
            if touched > 0 && !options.dry_run && !options.remove {
                println!(
                    "\nDone. Restart your editor or agent, then ask it to read a file with anymd."
                );
            }
            0
        }
        Err(error) => {
            eprintln!("anymd setup: {error}");
            1
        }
    }
}

fn split_clients(list: &str) -> Vec<String> {
    list.split(',')
        .map(|client| client.trim().to_string())
        .collect()
}

fn usage_error(message: &str) -> i32 {
    eprintln!(
        "anymd setup: {message}\nUsage: anymd setup [--dry-run] [--remove] [--client=<id,...>]"
    );
    2
}
