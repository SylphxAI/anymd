use std::path::Path;
use std::process::{Command, Output};

fn cli(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anymd"))
        .current_dir(directory)
        .args(arguments)
        .env("ANYMD_CACHE_DIR", directory.join("cache"))
        .output()
        .unwrap()
}

#[test]
fn failed_conversion_preserves_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("report.md");
    std::fs::write(&output, "Previous report\n").unwrap();
    std::fs::write(directory.path().join("corrupt.docx"), "not a document").unwrap();
    for source in ["missing.pdf", "corrupt.docx"] {
        let result = cli(directory.path(), &[source, "--output", "report.md"]);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains(source));
        assert_eq!(
            std::fs::read_to_string(&output).unwrap(),
            "Previous report\n"
        );
    }
}

#[test]
fn failed_conversion_does_not_create_output() {
    let directory = tempfile::tempdir().unwrap();
    let result = cli(directory.path(), &["missing.pdf", "-o", "report.md"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(!directory.path().join("report.md").exists());
}

#[test]
fn partial_conversion_preserves_output_but_remains_available_on_stdout() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("report.md");
    std::fs::write(directory.path().join("valid.txt"), "New report\n").unwrap();
    std::fs::write(&output, "Previous report\n").unwrap();
    for inputs in [["valid.txt", "missing.pdf"], ["missing.pdf", "valid.txt"]] {
        let result = cli(directory.path(), &[inputs[0], inputs[1], "-o", "report.md"]);
        assert_eq!(result.status.code(), Some(1));
        assert_eq!(
            std::fs::read_to_string(&output).unwrap(),
            "Previous report\n"
        );

        let stdout = cli(directory.path(), &inputs);
        assert_eq!(stdout.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&stdout.stdout).contains("New report"));
    }
}

#[test]
fn successful_conversion_replaces_output() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("report.md");
    std::fs::write(directory.path().join("valid.txt"), "New report\n").unwrap();
    std::fs::write(&output, "Previous report\n").unwrap();
    let result = cli(directory.path(), &["valid.txt", "-o", "report.md"]);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "New report\n");
}

#[test]
fn end_of_options_allows_dash_prefixed_paths_and_literal_help_flags() {
    let directory = tempfile::tempdir().unwrap();
    for source in ["-notes.txt", "--help", "--version"] {
        std::fs::write(directory.path().join(source), "Literal document\n").unwrap();
        let result = cli(directory.path(), &["--", source]);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, b"Literal document\n");
    }
    for option in ["--help", "--version"] {
        let result = cli(directory.path(), &[option]);
        assert!(result.status.success());
        assert!(String::from_utf8_lossy(&result.stdout).contains("anymd"));
    }
}
