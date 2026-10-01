//! Model-free process tests. Execute only on an isolated CI runner, never the desk.
#[path = "../src/command_provider.rs"]
#[allow(dead_code)]
mod command_provider;

use std::io::Write;
use std::time::Duration;

fn main() {
    if std::env::var("ANYMD_TEST_WORKER_LIFECYCLE").as_deref() != Ok("1") {
        println!("worker lifecycle tests require the isolated Doc-VLM CI gate");
        return;
    }
    assert_eq!(
        std::env::var("CI").as_deref(),
        Ok("true"),
        "isolated CI only"
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--worker") {
        if let Some(timeout) = args
            .last()
            .and_then(|arg| arg.strip_prefix("--supervised="))
        {
            command_provider::supervise_parent(timeout.parse().unwrap()).unwrap();
        }
        let mut heartbeat = std::fs::File::create(&args[1]).unwrap();
        writeln!(heartbeat, "{}", std::process::id()).unwrap();
        heartbeat.flush().unwrap();
        loop {
            writeln!(heartbeat, "alive").unwrap();
            heartbeat.flush().unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if args.first().is_some_and(|arg| !arg.starts_with('-')) {
        // Python convert passes its local source followed by normal CLI options.
        // Exercise the exact native runner, which creates a separate group/job.
        let source = std::path::Path::new(&args[0]);
        std::fs::write(
            source.with_extension("parent"),
            std::process::id().to_string(),
        )
        .unwrap();
        let result = command_provider::run_supervised(command_provider::CommandInvocation {
            command: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            args: vec![
                "--worker".into(),
                source
                    .with_extension("worker")
                    .to_string_lossy()
                    .into_owned(),
            ],
            timeout_ms: 30_000,
            max_stdout_bytes: 1024,
            failure_message: "worker failed".into(),
            timeout_message: "worker timeout".into(),
        });
        panic!("fixture worker unexpectedly returned: {result:?}");
    }
    // Custom harness keeps helper modes available without a production CLI hook.
    assert_eq!(
        std::env::var("CI").as_deref(),
        Ok("true"),
        "isolated CI only"
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = std::process::Command::new("python")
        .arg(root.join("crates/anymd/tests/worker_lifecycle.py"))
        .env("ANYMD_LIFECYCLE_BIN", std::env::current_exe().unwrap())
        .env("PYTHONPATH", root.join("packages/pypi"))
        .status()
        .unwrap();
    assert!(status.success(), "worker lifecycle regression failed");
}
