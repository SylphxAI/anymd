//! Process-local LRU for identical local `read_pdf` requests.
//!
//! Persistent MCP servers often re-read the same path+options. Caching the full
//! successful source result makes warm tools/call latency reflect incremental
//! request handling instead of re-parsing the entire PDF every time.
//!
//! Safety:
//! - key includes absolute path, mtime, size, and a request fingerprint
//! - only successful local path results are stored
//! - bounded entry count; never caches URL/temp fetches

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::read_pdf::{ReadPdfInput, ReadPdfSourceResult};

const MAX_ENTRIES: usize = 32;

#[derive(Clone)]
struct CacheEntry {
    key: String,
    result: ReadPdfSourceResult,
    last_used: u64,
}

#[derive(Default)]
struct CacheState {
    entries: HashMap<String, CacheEntry>,
    clock: u64,
}

static CACHE: OnceLock<Mutex<CacheState>> = OnceLock::new();

fn cache() -> &'static Mutex<CacheState> {
    CACHE.get_or_init(|| Mutex::new(CacheState::default()))
}

fn path_identity(path: &Path) -> Option<(PathBuf, u64, SystemTime)> {
    let meta = fs::metadata(path).ok()?;
    let abs = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let modified = meta.modified().ok()?;
    Some((abs, meta.len(), modified))
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn make_key(path: &Path, input: &ReadPdfInput, pages_spec: &Option<Value>) -> Option<String> {
    let (abs, len, modified) = path_identity(path)?;
    let mtime = modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let mut hasher = Sha256::new();
    let payload = serde_json::json!({
        "pages": pages_spec,
        "auto": input.auto,
        "auto_detail": input.auto_detail,
        "auto_policy_resolved": input.auto_policy_resolved,
        "sample_pages": input.sample_pages,
        "include_metadata": input.include_metadata,
        "include_page_count": input.include_page_count,
        "include_full_text": input.include_full_text,
        "include_markdown": input.include_markdown,
        "include_chunks": input.include_chunks,
        "include_elements": input.include_elements,
        "include_text_layer": input.include_text_layer,
        "include_document_map": input.include_document_map,
        "include_images": input.include_images,
        "include_tables": input.include_tables,
        "include_html": input.include_html,
        "include_semantic_hints": input.include_semantic_hints,
        "include_outline": input.include_outline,
        "include_annotations": input.include_annotations,
        "include_page_labels": input.include_page_labels,
        "include_page_geometry": input.include_page_geometry,
        "include_permissions": input.include_permissions,
        "include_form_fields": input.include_form_fields,
        "include_attachments": input.include_attachments,
        "include_structure_tree": input.include_structure_tree,
        "include_safety_findings": input.include_safety_findings,
        "include_layout_diagnostics": input.include_layout_diagnostics,
        "include_document_ast": input.include_document_ast,
        "include_ocr_text_layer": input.include_ocr_text_layer,
        "include_visual_enrichments": input.include_visual_enrichments,
        "include_trust_report": input.include_trust_report,
        "include_accessibility_report": input.include_accessibility_report,
        "trust_report_redaction": input.trust_report_redaction,
        "max_visual_enrichments": input.max_visual_enrichments,
    });
    hasher.update(payload.to_string().as_bytes());
    let fp = hex_encode(hasher.finalize());
    Some(format!("{}|{}|{}|{}", abs.display(), len, mtime, fp))
}

pub fn get_cached_local_result(
    path: &Path,
    input: &ReadPdfInput,
    pages_spec: &Option<Value>,
) -> Option<ReadPdfSourceResult> {
    let key = make_key(path, input, pages_spec)?;
    let mut state = cache().lock().ok()?;
    state.clock = state.clock.saturating_add(1);
    let now = state.clock;
    let entry = state.entries.get_mut(&key)?;
    entry.last_used = now;
    Some(entry.result.clone())
}

pub fn store_cached_local_result(
    path: &Path,
    input: &ReadPdfInput,
    pages_spec: &Option<Value>,
    result: &ReadPdfSourceResult,
) {
    if !result.success || result.data.is_none() {
        return;
    }
    let Some(key) = make_key(path, input, pages_spec) else {
        return;
    };
    let Ok(mut state) = cache().lock() else {
        return;
    };
    state.clock = state.clock.saturating_add(1);
    let now = state.clock;
    state.entries.insert(
        key.clone(),
        CacheEntry {
            key,
            result: result.clone(),
            last_used: now,
        },
    );
    while state.entries.len() > MAX_ENTRIES {
        let victim = state
            .entries
            .values()
            .min_by_key(|entry| entry.last_used)
            .map(|entry| entry.key.clone());
        if let Some(key) = victim {
            state.entries.remove(&key);
        } else {
            break;
        }
    }
}

#[cfg(test)]
pub fn clear_for_tests() {
    if let Ok(mut state) = cache().lock() {
        state.entries.clear();
        state.clock = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_pdf::{EngineInfo, ReadPdfData, ReadPdfInput, ReadPdfSourceResult};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn caches_identical_local_requests() {
        clear_for_tests();
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "%PDF-1.4").unwrap();
        let path = file.path();
        let input = ReadPdfInput {
            include_tables: true,
            include_page_count: true,
            ..Default::default()
        };
        let pages = None;
        assert!(get_cached_local_result(path, &input, &pages).is_none());
        let result = ReadPdfSourceResult {
            source: path.display().to_string(),
            success: true,
            error: None,
            data: Some(ReadPdfData {
                num_pages: Some(1),
                info: None,
                metadata: None,
                full_text: None,
                page_texts: None,
                markdown: None,
                html: None,
                chunks: None,
                elements: None,
                text_layer: None,
                tables: Some(serde_json::json!([])),
                images: None,
                safety_findings: None,
                layout_diagnostics: None,
                document_map: None,
                document_ast: None,
                trust_report: None,
                accessibility_report: None,
                outline: None,
                annotations: None,
                form_fields: None,
                attachments: None,
                structure_trees: None,
                page_labels: None,
                page_geometry: None,
                permissions: None,
                mark_info: None,
                ocr_text_layer: None,
                visual_enrichments: None,
                visual_enrichment_candidates: None,
                warnings: None,
                route: "test".into(),
                engine: EngineInfo {
                    name: "test",
                    version: "0",
                },
                ocr_candidate_pages: vec![],
                structured_fusion_context: None,
            }),
        };
        store_cached_local_result(path, &input, &pages, &result);
        let hit = get_cached_local_result(path, &input, &pages).expect("cache hit");
        assert!(hit.success);
        assert_eq!(hit.data.as_ref().unwrap().num_pages, Some(1));
    }
}
