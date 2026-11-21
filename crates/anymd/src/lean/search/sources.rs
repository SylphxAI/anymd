//! Expand and open the set of files and URLs to search.

use std::path::{Path, PathBuf};

use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;

use crate::document::{is_url, searchable_extension, OpenOptions, Opened, Unit};
use crate::source_access::SourceAccessPolicy;

const MAX_SEARCH_FILES: usize = 2000;

pub(super) struct SearchDoc {
    pub(super) label: String,
    pub(super) noun: &'static str,
    pub(super) total: u32,
    pub(super) units: std::sync::Arc<Vec<Unit>>,
    pub(super) outline: crate::outline::Outline,
}

pub(super) fn expand_sources(
    specs: &[String],
    policy: &SourceAccessPolicy,
    glob: Option<&str>,
    notes: &mut Vec<String>,
) -> Result<Vec<(String, String)>, String> {
    // (display label, spec to open)
    let mut files = Vec::new();
    for spec in specs {
        let spec = spec.trim();
        if is_url(spec) {
            files.push((spec.to_string(), spec.to_string()));
            continue;
        }
        let admitted = policy.admit_path(spec)?;
        let root = PathBuf::from(&admitted);
        if !root.is_dir() {
            files.push((spec.to_string(), admitted));
            continue;
        }
        let mut builder = WalkBuilder::new(&root);
        builder.follow_links(false);
        if let Some(glob) = glob {
            let mut overrides = OverrideBuilder::new(&root);
            overrides
                .add(glob)
                .map_err(|error| format!("invalid glob {glob:?}: {error}"))?;
            builder.overrides(overrides.build().map_err(|error| error.to_string())?);
        }
        let mut count = 0usize;
        for entry in builder.build().flatten() {
            let path = entry.path();
            if !path.is_file() || !searchable_extension(path) {
                continue;
            }
            if files.len() >= MAX_SEARCH_FILES {
                count += 1;
                continue;
            }
            let shown = Path::new(spec)
                .join(path.strip_prefix(&root).unwrap_or(path))
                .display()
                .to_string();
            files.push((shown, path.display().to_string()));
        }
        if count > 0 {
            notes.push(format!(
                "{count} files under {spec} were not searched (limit {MAX_SEARCH_FILES})."
            ));
        }
    }
    Ok(files)
}

pub(super) fn load_search_docs(
    files: Vec<(String, String)>,
    policy: &SourceAccessPolicy,
    notes: &mut Vec<String>,
) -> Vec<SearchDoc> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8)
        .min(files.len().max(1));
    let options = OpenOptions {
        ocr: Some(false),
        ocr_engine: None,
        transcript: false,
        download_asr_model: false,
        images: None,
        revisions: anymd_formats::Revisions::default(),
    };
    let mut results: Vec<Option<Result<SearchDoc, String>>> =
        (0..files.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let files = &files;
                let options = &options;
                scope.spawn(move || {
                    files
                        .iter()
                        .enumerate()
                        .skip(worker)
                        .step_by(workers)
                        .map(|(index, (label, spec))| {
                            let doc = Opened::open(spec, policy, options).and_then(|opened| {
                                let units = opened.all_units()?;
                                let outline = crate::outline::Outline::from_units(&opened, &units);
                                Ok(SearchDoc {
                                    label: label.clone(),
                                    noun: opened.unit_noun,
                                    total: opened.total,
                                    units,
                                    outline,
                                })
                            });
                            (index, doc)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            if let Ok(done) = handle.join() {
                for (index, doc) in done {
                    results[index] = Some(doc);
                }
            }
        }
    });
    let mut docs = Vec::new();
    for (result, (label, _)) in results.into_iter().zip(&files) {
        match result {
            Some(Ok(doc)) => docs.push(doc),
            Some(Err(message)) => notes.push(format!("{label}: {message}")),
            None => notes.push(format!("{label}: failed to open")),
        }
    }
    docs
}
