//! Run the released binary shape with a real PTY connected only to stderr.

#[cfg(target_os = "linux")]
#[test]
fn real_terminal_keeps_cli_and_mcp_output_intact() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = std::process::Command::new("python3")
        .arg(root.join("crates/anymd/tests/pro_hint.py"))
        .arg(env!("CARGO_BIN_EXE_anymd"))
        .arg(&root)
        .status()
        .expect("Python 3 is required for the Linux PTY test");
    assert!(status.success(), "CLI Pro hint PTY regression");
}
