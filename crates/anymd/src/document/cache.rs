use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anymd_formats::images::ImageStore;

use super::{OpenOptions, Unit};

const CACHE_MAX_BYTES: usize = 256 * 1024 * 1024;

/// Path, size, mtime, OCR on, and the image store (its Markdown points there).
pub(super) type CacheKey = (
    PathBuf,
    u64,
    u128,
    bool,
    Option<bool>,
    crate::ocr_vlm::OcrEngine,
    bool,
    String,
    Option<PathBuf>,
    anymd_formats::Revisions,
);

pub(super) struct CachedDoc {
    pub(super) format: &'static str,
    pub(super) native_outline: Vec<(usize, String, Option<u32>)>,
    pub(super) title: Option<String>,
    pub(super) metadata: Vec<(String, String)>,
    pub(super) units: Arc<Vec<Unit>>,
    pub(super) bytes: usize,
    /// Exported image files the Markdown points at.
    pub(super) images: Vec<PathBuf>,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<CacheKey, Arc<CachedDoc>>,
    order: VecDeque<CacheKey>,
    bytes: usize,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

pub(super) fn cache_key(path: &Path, ocr: bool, options: &OpenOptions) -> Option<CacheKey> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((
        path.to_path_buf(),
        meta.len(),
        modified,
        ocr,
        options.ocr,
        options.ocr_engine.unwrap_or_default(),
        crate::ocr_vlm::root().is_ok_and(|p| crate::ocr_vlm::ready_at(&p)),
        format!(
            "{:?}|{:?}|{:?}",
            crate::ocr_vlm::root(),
            std::env::var("ANYMD_OCR_QUANTIZATION").ok(),
            std::env::var("ANYMD_OCR_MAX_TOKENS").ok()
        ),
        options.images.as_ref().map(|s| s.dir().to_path_buf()),
        options.revisions,
    ))
}

pub(super) fn cache_get(key: &CacheKey) -> Option<Arc<CachedDoc>> {
    let doc = cache().lock().ok()?.entries.get(key).cloned()?;
    // The Markdown points at files in the image cache, which may have been
    // pruned or deleted since: a missing file makes this a miss, and the
    // rebuild re-exports it. Files still there are marked as in use.
    if doc.images.iter().any(|path| !path.is_file()) {
        return None;
    }
    doc.images
        .iter()
        .for_each(|path| anymd_formats::cache::touch(path));
    Some(doc)
}

/// The exported image files the units' Markdown points at.
pub(super) fn referenced_images(units: &[Unit], store: Option<&ImageStore>) -> Vec<PathBuf> {
    let Some(store) = store else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for unit in units {
        for line in unit.markdown.lines().filter(|l| l.starts_with("![")) {
            let Some((_, rest)) = line.rsplit_once("](") else {
                continue;
            };
            let path = rest.strip_suffix(')').unwrap_or(rest);
            let path = PathBuf::from(path.trim_start_matches('<').trim_end_matches('>'));
            if path.starts_with(store.dir()) && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

pub(super) fn cache_put(key: CacheKey, doc: Arc<CachedDoc>) {
    let Ok(mut cache) = cache().lock() else {
        return;
    };
    if doc.bytes > CACHE_MAX_BYTES / 4 {
        return;
    }
    if let Some(old) = cache.entries.insert(key.clone(), doc.clone()) {
        cache.bytes -= old.bytes;
        cache.order.retain(|k| k != &key);
    }
    cache.bytes += doc.bytes;
    cache.order.push_back(key);
    while cache.bytes > CACHE_MAX_BYTES {
        let Some(oldest) = cache.order.pop_front() else {
            break;
        };
        if let Some(evicted) = cache.entries.remove(&oldest) {
            cache.bytes -= evicted.bytes;
        }
    }
}
