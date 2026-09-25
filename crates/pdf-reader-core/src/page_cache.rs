//! Persisted page-text cache keyed by source hash for repeated literal search.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hash_file;
use crate::text_index::{extract_page_texts, TextIndexError};

pub const PAGE_CACHE_SCHEMA_VERSION: &str = "2.1.0-inferred-word-spaces";
pub const PAGE_CACHE_DIR: &str = ".pdf-reader-mcp/page-cache";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTextCacheEntry {
    pub schema_version: String,
    pub source_hash: String,
    pub path: String,
    pub pages: Vec<String>,
    pub page_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageCacheStatus {
    CacheHit,
    CacheMiss,
    CacheBypass,
}

pub fn page_cache_path(pdf_path: &Path, source_hash: &str) -> PathBuf {
    pdf_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(PAGE_CACHE_DIR)
        .join(format!("{source_hash}.json"))
}

pub fn load_cached_pages(pdf_path: &Path, source_hash: &str) -> Option<Vec<String>> {
    let cache_path = page_cache_path(pdf_path, source_hash);
    let raw = fs::read_to_string(&cache_path).ok()?;
    let entry: PageTextCacheEntry = serde_json::from_str(&raw).ok()?;
    if entry.schema_version != PAGE_CACHE_SCHEMA_VERSION || entry.source_hash != source_hash {
        return None;
    }
    Some(entry.pages)
}

pub fn store_cached_pages(
    pdf_path: &Path,
    source_hash: &str,
    pages: &[String],
) -> Result<PathBuf, String> {
    let cache_path = page_cache_path(pdf_path, source_hash);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create page cache directory: {error}"))?;
    }

    let entry = PageTextCacheEntry {
        schema_version: PAGE_CACHE_SCHEMA_VERSION.into(),
        source_hash: source_hash.into(),
        path: pdf_path.to_string_lossy().into_owned(),
        pages: pages.to_vec(),
        page_count: pages.len() as u32,
    };

    let serialized = serde_json::to_string(&entry)
        .map_err(|error| format!("Failed to serialize page cache entry: {error}"))?;
    fs::write(&cache_path, serialized)
        .map_err(|error| format!("Failed to write page cache entry: {error}"))?;
    Ok(cache_path)
}

pub fn extract_page_texts_cached(
    path: &Path,
    max_file_bytes: u64,
) -> Result<(Vec<String>, PageCacheStatus), TextIndexError> {
    let hash = hash_file(path, max_file_bytes)?;
    if let Some(pages) = load_cached_pages(path, &hash.source_hash) {
        return Ok((pages, PageCacheStatus::CacheHit));
    }

    let pages = extract_page_texts(path, max_file_bytes)?;
    if store_cached_pages(path, &hash.source_hash, &pages).is_ok() {
        return Ok((pages, PageCacheStatus::CacheMiss));
    }

    Ok((pages, PageCacheStatus::CacheBypass))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn stores_and_reloads_page_text_by_source_hash() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pdf_path = temp.path().join("sample.pdf");
        let mut file = fs::File::create(&pdf_path).expect("create");
        writeln!(file, "%PDF-1.4").expect("write");
        writeln!(file, "Hello\x0cWorld").expect("write");

        let hash = hash_file(&pdf_path, 1024 * 1024).expect("hash");
        let pages = vec!["Hello".into(), "World".into()];
        let cache_path = store_cached_pages(&pdf_path, &hash.source_hash, &pages).expect("store");

        assert!(cache_path.exists());
        let loaded = load_cached_pages(&pdf_path, &hash.source_hash).expect("load");
        assert_eq!(loaded, pages);
    }

    #[test]
    fn invalidates_cache_when_source_hash_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pdf_path = temp.path().join("sample.pdf");
        fs::write(&pdf_path, b"v1").expect("write");
        let hash = hash_file(&pdf_path, 1024 * 1024).expect("hash");
        store_cached_pages(&pdf_path, &hash.source_hash, &["one".into()]).expect("store");

        fs::write(&pdf_path, b"v2-changed").expect("rewrite");
        let new_hash = hash_file(&pdf_path, 1024 * 1024).expect("hash");
        assert!(load_cached_pages(&pdf_path, &new_hash.source_hash).is_none());
    }

    #[test]
    fn bulk_page_cache_path_uses_parent_and_hash() {
        use std::path::Path;
        let p = page_cache_path(Path::new("/data/docs/file.pdf"), "abc123");
        assert!(p.to_string_lossy().contains("abc123.json"));
        assert!(p.to_string_lossy().contains("page-cache"));
        let p2 = page_cache_path(Path::new("file.pdf"), "h");
        assert!(p2.to_string_lossy().contains("h.json"));
    }
}
