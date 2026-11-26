use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::path::{resolve_allowed_dir, resolve_source_path};
use crate::schema::{PdfEvidenceSource, PdfSource};

const ALLOWED_DIRS_ENV: &str = "MCP_PDF_ALLOWED_DIRS";

/// Filesystem admission policy for caller-supplied and derived source paths.
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
            if argument == "--" {
                break;
            }
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
            let canonical = resolve_allowed_dir(&startup_cwd, root)?;
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

        let canonical =
            resolve_source_path(&self.startup_cwd, Path::new(user_path)).map_err(|error| {
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

impl anymd_formats::SourceAccess for SourceAccessPolicy {
    fn admit_path(&self, path: &Path) -> Result<PathBuf, String> {
        let path = path
            .to_str()
            .ok_or("Access denied: path does not have a valid UTF-8 representation.")?;
        SourceAccessPolicy::admit_path(self, path).map(PathBuf::from)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
