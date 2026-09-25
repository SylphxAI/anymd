use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::schema::{PdfEvidenceSource, PdfSource};

const ALLOWED_DIRS_ENV: &str = "MCP_PDF_ALLOWED_DIRS";

/// Filesystem admission policy for caller-supplied PDF paths.
///
/// URL-backed temporary files are materialized after this boundary and are not
/// caller-supplied paths. With no configured roots, historical unrestricted
/// filesystem behavior is preserved.
#[derive(Clone, Debug)]
pub struct SourceAccessPolicy {
    startup_cwd: Arc<PathBuf>,
    allowed_dirs: Option<Arc<[PathBuf]>>,
}

impl Default for SourceAccessPolicy {
    fn default() -> Self {
        Self::unrestricted()
    }
}

impl SourceAccessPolicy {
    pub fn unrestricted() -> Self {
        let startup_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            startup_cwd: Arc::new(startup_cwd),
            allowed_dirs: None,
        }
    }

    pub fn from_process() -> Result<Self, String> {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let env_value = std::env::var(ALLOWED_DIRS_ENV).ok();
        let startup_cwd = std::env::current_dir()
            .map_err(|error| format!("Failed to resolve the server startup directory: {error}"))?;
        Self::from_inputs(&args, env_value.as_deref(), startup_cwd)
    }

    fn from_inputs(
        args: &[OsString],
        env_value: Option<&str>,
        startup_cwd: PathBuf,
    ) -> Result<Self, String> {
        let mut roots = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let argument = args[index].to_string_lossy();
            if argument == "--allow-dir" {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    "--allow-dir requires a directory path (or use --allow-dir=<path>).".to_string()
                })?;
                if value.is_empty() {
                    return Err("--allow-dir requires a non-empty directory path.".into());
                }
                roots.push(PathBuf::from(value));
            } else if let Some(value) = argument.strip_prefix("--allow-dir=") {
                if value.is_empty() {
                    return Err("--allow-dir requires a non-empty directory path.".into());
                }
                roots.push(PathBuf::from(value));
            }
            index += 1;
        }

        if let Some(raw) = env_value {
            for comma_group in raw
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                roots.extend(
                    std::env::split_paths(comma_group)
                        .filter(|value| !value.as_os_str().is_empty()),
                );
            }
        }

        if roots.is_empty() {
            return Ok(Self {
                startup_cwd: Arc::new(startup_cwd),
                allowed_dirs: None,
            });
        }

        let mut canonical_roots = Vec::with_capacity(roots.len());
        for root in roots {
            let absolute = if root.is_absolute() {
                root
            } else {
                startup_cwd.join(root)
            };
            let canonical = std::fs::canonicalize(&absolute).map_err(|error| {
                format!(
                    "Failed to resolve allowed directory '{}': {error}",
                    absolute.display()
                )
            })?;
            if !canonical.is_dir() {
                return Err(format!(
                    "Allowed directory '{}' is not a directory.",
                    canonical.display()
                ));
            }
            if !canonical_roots.contains(&canonical) {
                canonical_roots.push(canonical);
            }
        }

        Ok(Self {
            startup_cwd: Arc::new(startup_cwd),
            allowed_dirs: Some(canonical_roots.into()),
        })
    }

    pub fn is_restricted(&self) -> bool {
        self.allowed_dirs.is_some()
    }

    pub fn allowed_dir_count(&self) -> usize {
        self.allowed_dirs.as_deref().map_or(0, <[PathBuf]>::len)
    }

    #[cfg(test)]
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

    pub fn admit_pdf_sources(&self, sources: &mut [PdfSource]) -> Result<(), String> {
        for source in sources {
            self.admit_optional_path(&mut source.path)?;
        }
        Ok(())
    }

    pub fn admit_evidence_sources(&self, sources: &mut [PdfEvidenceSource]) -> Result<(), String> {
        for source in sources {
            self.admit_optional_path(&mut source.path)?;
        }
        Ok(())
    }

    /// Admit one local path (or pass a URL through); returns the path to open.
    pub fn admit_path(&self, path: &str) -> Result<String, String> {
        let mut slot = Some(path.to_string());
        self.admit_optional_path(&mut slot)?;
        Ok(slot.unwrap_or_default())
    }

    fn admit_optional_path(&self, path: &mut Option<String>) -> Result<(), String> {
        let Some(allowed_dirs) = self.allowed_dirs.as_deref() else {
            return Ok(());
        };
        let Some(user_path) = path.as_deref() else {
            return Ok(());
        };

        let requested = Path::new(user_path);
        let absolute = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.startup_cwd.join(requested)
        };
        let canonical = canonicalize_with_missing_tail(&absolute).map_err(|error| {
            format!("Access denied: path '{user_path}' could not be resolved: {error}")
        })?;

        if !allowed_dirs.iter().any(|root| canonical.starts_with(root)) {
            return Err(format!(
                "Access denied: path '{user_path}' is outside the configured allowed directories."
            ));
        }

        let admitted = canonical.to_str().ok_or_else(|| {
            format!("Access denied: path '{user_path}' does not have a valid UTF-8 representation.")
        })?;
        *path = Some(admitted.to_string());
        Ok(())
    }
}

fn canonicalize_with_missing_tail(path: &Path) -> std::io::Result<PathBuf> {
    let mut current = path;
    let mut missing = Vec::new();

    loop {
        match std::fs::canonicalize(current) {
            Ok(mut canonical) => {
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                let Some(name) = current.file_name() else {
                    return Err(error);
                };
                missing.push(name.to_os_string());
                let Some(parent) = current.parent() else {
                    return Err(error);
                };
                current = parent;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SourceAccessPolicy;
    use crate::schema::PdfSource;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

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
        fs::write(&existing, b"pdf").expect("fixture");
        let args = vec![OsString::from(format!("--allow-dir={}", root.display()))];
        let policy = SourceAccessPolicy::from_inputs(&args, None, temp.path().to_path_buf())
            .expect("policy");
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
                .is_some_and(|path| PathBuf::from(path).starts_with(&root))
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
        let policy = SourceAccessPolicy::from_inputs(&args, None, temp.path().to_path_buf())
            .expect("policy");

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
        let policy = SourceAccessPolicy::from_inputs(&args, None, temp.path().to_path_buf())
            .expect("policy");
        let mut sources = vec![source(root.join("escape/secret.pdf").to_string_lossy())];
        assert!(policy.admit_pdf_sources(&mut sources).is_err());
    }
}
