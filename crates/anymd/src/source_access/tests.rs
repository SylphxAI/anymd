use super::SourceAccessPolicy;
use crate::schema::PdfSource;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

impl SourceAccessPolicy {
    pub(crate) fn restricted_for_test(
        startup_cwd: PathBuf,
        allowed_dir: &Path,
    ) -> Result<Self, String> {
        let args = vec![OsString::from(format!(
            "--allow-dir={}",
            allowed_dir.display()
        ))];
        Self::from_inputs(&args, None, startup_cwd)
    }
}

fn source(path: impl Into<String>) -> PdfSource {
    PdfSource {
        path: Some(path.into()),
        url: None,
        pages: None,
    }
}

#[test]
fn default_is_unrestricted_and_preserves_relative_path() {
    let policy = SourceAccessPolicy::from_inputs(&[], None, PathBuf::from("/unused"))
        .expect("unrestricted policy");
    let mut sources = vec![source("relative.pdf")];
    policy.admit_pdf_sources(&mut sources).expect("admitted");
    assert!(!policy.is_restricted());
    assert_eq!(sources[0].path.as_deref(), Some("relative.pdf"));
}

#[test]
fn cli_and_environment_roots_merge_and_canonicalize() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::create_dir_all(&first).expect("first root");
    fs::create_dir_all(&second).expect("second root");
    let env_value = std::env::join_paths([&second])
        .expect("join paths")
        .to_string_lossy()
        .to_string();
    let args = vec![OsString::from(format!("--allow-dir={}", first.display()))];
    let policy =
        SourceAccessPolicy::from_inputs(&args, Some(&env_value), temp.path().to_path_buf())
            .expect("merged policy");
    assert!(policy.is_restricted());
    assert_eq!(policy.allowed_dir_count(), 2);
}

#[test]
fn exact_root_descendant_and_missing_descendant_are_admitted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("allowed");
    fs::create_dir_all(root.join("nested")).expect("allowed tree");
    let existing = root.join("nested/existing.pdf");
    // Admitted paths are canonical (/private/var on macOS, \\?\ on Windows).
    let canonical_root = fs::canonicalize(&root).expect("canonical root");
    fs::write(&existing, b"pdf").expect("fixture");
    let args = vec![OsString::from(format!("--allow-dir={}", root.display()))];
    let policy =
        SourceAccessPolicy::from_inputs(&args, None, temp.path().to_path_buf()).expect("policy");
    let mut sources = vec![
        source(root.to_string_lossy()),
        source(existing.to_string_lossy()),
        source("allowed/nested/missing.pdf"),
    ];
    policy.admit_pdf_sources(&mut sources).expect("admitted");
    assert!(sources.iter().all(|source| {
        source
            .path
            .as_deref()
            .is_some_and(|path| PathBuf::from(path).starts_with(&canonical_root))
    }));
}

#[test]
fn sibling_prefix_and_parent_traversal_are_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("allowed");
    let sibling = temp.path().join("allowed-sibling");
    fs::create_dir_all(&root).expect("allowed root");
    fs::create_dir_all(&sibling).expect("sibling root");
    let args = vec![OsString::from(format!("--allow-dir={}", root.display()))];
    let policy =
        SourceAccessPolicy::from_inputs(&args, None, temp.path().to_path_buf()).expect("policy");

    let mut sibling_source = vec![source(sibling.join("outside.pdf").to_string_lossy())];
    assert!(policy.admit_pdf_sources(&mut sibling_source).is_err());

    let mut traversal_source = vec![source("allowed/../outside.pdf")];
    assert!(policy.admit_pdf_sources(&mut traversal_source).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("allowed");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&root).expect("allowed root");
    fs::create_dir_all(&outside).expect("outside root");
    fs::write(outside.join("secret.pdf"), b"pdf").expect("outside fixture");
    symlink(&outside, root.join("escape")).expect("symlink");
    let args = vec![OsString::from(format!("--allow-dir={}", root.display()))];
    let policy =
        SourceAccessPolicy::from_inputs(&args, None, temp.path().to_path_buf()).expect("policy");
    let mut sources = vec![source(root.join("escape/secret.pdf").to_string_lossy())];
    assert!(policy.admit_pdf_sources(&mut sources).is_err());
}
