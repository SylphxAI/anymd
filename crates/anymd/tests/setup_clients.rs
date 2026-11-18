//! Exercise client validation through the executable, not just the kit API.
//! On Unix, HOME and XDG_CONFIG_HOME isolate the kit's client config paths.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let contents = if entry.file_type().unwrap().is_dir() {
                pending.push(path.clone());
                None
            } else {
                Some(fs::read(&path).unwrap())
            };
            entries.insert(path.strip_prefix(root).unwrap().to_owned(), contents);
        }
    }
    entries
}

#[test]
fn setup_rejects_unknown_clients_without_changing_config() {
    let home = tempfile::tempdir().unwrap();
    // dirs uses this path on macOS; point XDG_CONFIG_HOME here on Linux too.
    let config = home.path().join("Library/Application Support");
    fs::create_dir_all(config.join("Claude")).unwrap();
    fs::create_dir(home.path().join(".claude")).unwrap();
    let existing = br#"{"mcpServers":{"existing":{"command":"keep-me"}}}"#;
    fs::write(home.path().join(".claude.json"), existing).unwrap();
    fs::write(config.join("Claude/claude_desktop_config.json"), existing).unwrap();
    let before = snapshot(home.path());

    // A valid ID before an invalid one must not cause a partial write either.
    for (clients, unknown) in [
        ("claude", "claude"),
        ("foo", "foo"),
        ("claude-code,foo", "foo"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_anymd"))
            .args(["setup", "--client", clients])
            .current_dir(home.path())
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", &config)
            .env("PATH", home.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{clients}: {output:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains(&format!("Unknown MCP client ID {unknown:?}")),
            "{error}"
        );
        assert!(error.contains("Supported client IDs:"), "{error}");
        for id in ["claude-code", "claude-desktop"] {
            assert!(error.contains(id), "{error}");
        }
        if unknown == "claude" {
            assert!(
                error.contains("Did you mean \"claude-code\" or \"claude-desktop\"?"),
                "{error}"
            );
        } else {
            assert!(!error.contains("Did you mean"), "{error}");
        }
        assert_eq!(snapshot(home.path()), before, "{clients} changed config");
    }
}
