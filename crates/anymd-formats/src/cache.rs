//! The anymd cache directory: `ANYMD_CACHE_DIR`, else the platform cache
//! directory + `anymd`. Whisper models (`models/`) and exported document
//! images (`images/`) live under it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Overrides the cache directory.
pub const CACHE_ENV: &str = "ANYMD_CACHE_DIR";

/// The cache directory, or `None` when there is no home directory to derive one from.
pub fn cache_dir() -> Option<PathBuf> {
    cache_dir_from(&|key: &str| std::env::var_os(key))
}

pub(crate) fn cache_dir_from(get: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |key: &str| get(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(root) = var(CACHE_ENV) {
        return Some(root);
    }
    if cfg!(windows) {
        var("LOCALAPPDATA").map(|p| p.join("anymd").join("cache"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|p| p.join("Library").join("Caches").join("anymd"))
    } else {
        var("XDG_CACHE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|p| p.join(".cache")))
            .map(|p| p.join("anymd"))
    }
}

// ---------------------------------------------------------------------------
// Exported document images (`<cache>/images`)

/// Exported images unused for this long are deleted.
pub const IMAGE_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// The exported images together stay under this many bytes (oldest go first).
pub const IMAGE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 3600);
const STAMP: &str = ".images-pruned";

/// Mark a cache file as used now, so pruning keeps it. Best effort.
pub fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Delete files in `dir` older than `max_age`, then the oldest until the rest
/// fit in `max_bytes`. Dot files are left alone. Returns the files removed.
pub fn prune_dir(dir: &Path, max_age: Duration, max_bytes: u64, now: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_file()
                .then(|| (meta.modified().unwrap_or(now), meta.len(), entry.path()))
        })
        .collect();
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    let mut removed = 0;
    for (modified, len, path) in files {
        let old = now.duration_since(modified).is_ok_and(|age| age > max_age);
        if (old || total > max_bytes) && std::fs::remove_file(&path).is_ok() {
            total -= len;
            removed += 1;
        }
    }
    removed
}

/// Prune `<cache>/images` at most once a day (a stamp file in the cache
/// directory says when it last ran). Call at CLI and server start.
pub fn prune_images_daily() {
    let Some(root) = cache_dir() else { return };
    let stamp = root.join(STAMP);
    let now = SystemTime::now();
    let recent = std::fs::metadata(&stamp)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| now.duration_since(at).ok())
        .is_some_and(|age| age < PRUNE_INTERVAL);
    if recent || !root.join("images").is_dir() {
        return;
    }
    prune_dir(&root.join("images"), IMAGE_MAX_AGE, IMAGE_MAX_BYTES, now);
    let _ = std::fs::write(&stamp, b"");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, len: usize, age_days: u64, now: SystemTime) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, vec![0u8; len]).unwrap();
        let file = std::fs::File::options().append(true).open(&path).unwrap();
        file.set_modified(now - Duration::from_secs(age_days * 24 * 3600))
            .unwrap();
        path
    }

    #[test]
    fn prune_drops_old_files_then_oldest_over_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let stale = write(dir.path(), "stale.png", 10, 40, now);
        let a = write(dir.path(), "a.png", 100, 5, now);
        let b = write(dir.path(), "b.png", 100, 2, now);
        let dot = write(dir.path(), ".keep", 100, 90, now);
        assert_eq!(prune_dir(dir.path(), IMAGE_MAX_AGE, 150, now), 2);
        assert!(!stale.exists() && !a.exists());
        assert!(b.exists() && dot.exists());
    }

    #[test]
    fn touch_keeps_a_used_file() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let used = write(dir.path(), "used.png", 10, 40, now);
        touch(&used);
        assert_eq!(prune_dir(dir.path(), IMAGE_MAX_AGE, u64::MAX, now), 0);
        assert!(used.exists());
    }
}
