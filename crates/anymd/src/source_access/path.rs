use std::path::{Path, PathBuf};

pub(super) fn resolve_allowed_dir(startup_cwd: &Path, root: PathBuf) -> Result<PathBuf, String> {
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
    Ok(canonical)
}

pub(super) fn resolve_source_path(
    startup_cwd: &Path,
    requested: &Path,
) -> std::io::Result<PathBuf> {
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        startup_cwd.join(requested)
    };
    canonicalize_with_missing_tail(&absolute)
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
