//! Real, model-free CLI and MCP read checks for authored subtitle cues.
//! Verify installs FFmpeg before running the native suite. Other environments
//! without the optional media toolchain can still run the Rust parser tests.

use std::path::Path;
use std::process::Command;

#[test]
fn cli_and_mcp_preserve_the_original_media_corpus_and_sidecar_edge_cases() {
    if !Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        eprintln!("subtitle CLI/MCP fixture needs optional FFmpeg; parser tests still run");
        return;
    }
    let python = ["python3", "python"]
        .into_iter()
        .find(|python| {
            Command::new(python)
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .expect("Python 3 is required for the real subtitle fixture");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = Path::new(env!("CARGO_BIN_EXE_anymd"));
    let receipt = binary.parent().unwrap().join("subtitle-fidelity.json");
    let output = Command::new(python)
        .arg(root.join("scripts/subtitle_fidelity.py"))
        .arg("--binary")
        .arg(binary)
        .arg("--receipt")
        .arg(&receipt)
        .current_dir(&root)
        .output()
        .expect("run the real subtitle fixture");
    assert!(
        output.status.success(),
        "subtitle fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(receipt.is_file(), "the tested artifact must have a receipt");
}
