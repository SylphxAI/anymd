//! Directory listings of readable documents.

use std::path::Path;

use anymd_core::text_rules::yaml_value;
use ignore::WalkBuilder;

use crate::document::readable_extension;

pub(super) fn list_directory(label: &str, root: &Path) -> String {
    let mut files = Vec::new();
    let mut skipped = 0usize;
    for entry in WalkBuilder::new(root).max_depth(Some(6)).build().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if readable_extension(path) {
            if files.len() < 500 {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let shown = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string();
                files.push(format!("- {shown} ({})", human_size(size)));
            } else {
                skipped += 1;
            }
        }
    }
    let mut out = format!(
        "---\nsource: {}\ntype: directory\nreadable_files: {}\n---\n\n",
        yaml_value(label),
        files.len() + skipped
    );
    if files.is_empty() {
        out.push_str("No readable documents found (PDF, Office, EPUB, HTML, Markdown, CSV, images, media).\n");
    } else {
        out.push_str(&files.join("\n"));
        out.push('\n');
        if skipped > 0 {
            out.push_str(&format!("- … {skipped} more\n"));
        }
        out.push_str("\n<!-- Read one with read {source}, or search them all with search {query, sources: [this directory]}. -->\n");
    }
    out
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}
