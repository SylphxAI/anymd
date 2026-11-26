//! EPUB → Markdown: container.xml → OPF → spine order, one section per chapter.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use zip::ZipArchive;

use crate::html::{render_html, ImageRenderer, RenderOptions};
use crate::images::{self, Embed, ImageStore};
use crate::{ConvertError, Converted, Options, Section};

mod outline;
mod package;
#[cfg(test)]
pub(crate) mod tests;

use outline::{first_heading, has_text, native_outline};
use package::{
    join_path, parse_opf, percent_decode, read_entry, read_entry_bytes, rootfile_path, Archive,
    SharedArchive,
};

const MAX_ENTRIES: usize = 10_000;
const MAX_TOTAL_UNCOMPRESSED: u64 = 200 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 50 * 1024 * 1024;

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    convert_with_limits(bytes, options, crate::limits::Limits::default())
}

fn convert_with_limits(
    bytes: &[u8],
    options: &Options,
    limits: crate::limits::Limits,
) -> Result<Converted, ConvertError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ConvertError::Invalid(format!("not a valid EPUB zip: {e}")))?;
    crate::ooxml::validate_archive(bytes, &mut archive)?;
    if archive.len() > MAX_ENTRIES {
        return Err(ConvertError::Invalid(format!(
            "EPUB has {} entries (limit {MAX_ENTRIES})",
            archive.len()
        )));
    }
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|e| ConvertError::Invalid(format!("unreadable EPUB entry: {e}")))?;
        total = total.saturating_add(entry.size());
    }
    if total > MAX_TOTAL_UNCOMPRESSED {
        return Err(ConvertError::Invalid(format!(
            "EPUB expands to {total} bytes (limit {MAX_TOTAL_UNCOMPRESSED})"
        )));
    }

    let mut archive = Archive {
        zip: archive,
        limits,
    };
    let opf_path = match read_entry(&mut archive, "META-INF/container.xml") {
        Ok(container) => rootfile_path(&container),
        Err(_) => None,
    }
    .or_else(|| {
        archive
            .zip
            .file_names()
            .filter_map(|name| name.ok())
            .find(|n| n.to_ascii_lowercase().ends_with(".opf"))
            .map(|name| name.to_string())
    })
    .ok_or_else(|| ConvertError::Invalid("EPUB has no package document (OPF)".into()))?;
    let opf = read_entry(&mut archive, &opf_path)?;
    let package = parse_opf(&opf)?;
    let opf_dir = opf_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");

    // Retain paths, not one inflated copy per spine reference.
    let mut chapters = Vec::new();
    for entry in &package.spine {
        let Some(item) = package.manifest.get(&entry.idref) else {
            continue;
        };
        let navigation = item.properties.split_whitespace().any(|p| p == "nav");
        // A navigation document in the linear spine is also an authored page.
        // The native outline reads manifest navigation independently.
        if (navigation && !entry.linear)
            || !(item.media_type.contains("html") || item.media_type.is_empty())
        {
            continue;
        }
        let path = join_path(
            opf_dir,
            &percent_decode(item.href.split('#').next().unwrap_or("")),
        );
        chapters.push(Chapter { path, navigation });
    }

    let store = options.images.as_ref();
    let archive = RefCell::new(archive);
    // Pictures on three or more chapters (ornaments, publisher marks) are left out.
    let repeated = match store {
        Some(_) => repeated_pictures(&archive, &chapters),
        None => HashSet::new(),
    };
    archive.borrow().limits.check()?;
    let mut sections = Vec::new();
    let mut section_paths = Vec::new();
    for chapter in &chapters {
        let path = &chapter.path;
        let result = read_entry(&mut archive.borrow_mut(), path);
        let xhtml = match result {
            Ok(xhtml) => xhtml,
            Err(_) => {
                archive.borrow().limits.check()?;
                continue;
            }
        };
        let number = sections.len() + 1;
        let place = format!("chapter {number}");
        let dir = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let decide = |src: &str, alt: &str| match store {
            Some(store) => chapter_image(store, &archive, dir, src, alt, Some(&place), &repeated),
            None => Embed::Fallback,
        };
        let decide: &dyn Fn(&str, &str) -> Embed = &decide;
        let render = chapter.render_options(store.map(|_| decide));
        let markdown = render_html(&xhtml, &render)?;
        archive.borrow_mut().limits.output(&markdown)?;
        if !has_text(&markdown) && !markdown.contains("<!-- image:") {
            continue;
        }
        let label = match first_heading(&markdown) {
            Some(heading) => format!("chapter {number}: {heading}"),
            None => format!("chapter {number}"),
        };
        section_paths.push(path.clone());
        sections.push(Section { label, markdown });
    }
    if sections.is_empty() && package.spine.is_empty() {
        return Err(ConvertError::Invalid("EPUB spine is empty".into()));
    }

    let outline = native_outline(&archive, &package, opf_dir, &section_paths);
    archive.borrow().limits.check()?;
    let mut metadata = Vec::new();
    if !package.creators.is_empty() {
        metadata.push(("author".to_string(), package.creators.join("; ")));
    }
    for (key, value) in [
        ("publisher", package.publisher),
        ("date", package.date),
        ("language", package.language),
    ] {
        if let Some(value) = value {
            metadata.push((key.to_string(), value));
        }
    }
    metadata.push(("chapters".to_string(), sections.len().to_string()));
    Ok(Converted {
        outline,
        format: "epub".into(),
        title: package.title,
        sections,
        metadata,
    })
}

struct Chapter {
    path: String,
    navigation: bool,
}

impl Chapter {
    fn render_options<'a>(&self, images: Option<&'a ImageRenderer<'a>>) -> RenderOptions<'a> {
        RenderOptions {
            base: None,
            keep_relative_links: false,
            select_main: false,
            keep_nav: self.navigation,
            images,
        }
    }
}

/// The bytes of the image an `<img src>` in a chapter in `dir` names.
fn chapter_bytes(archive: &SharedArchive<'_>, dir: &str, src: &str) -> Option<Vec<u8>> {
    if src.contains("://") || src.starts_with("data:") {
        return None;
    }
    let target = src.split(['#', '?']).next().unwrap_or("");
    let path = join_path(dir, &percent_decode(target));
    read_entry_bytes(&mut archive.borrow_mut(), &path).ok()
}

fn chapter_image(
    store: &ImageStore,
    archive: &SharedArchive<'_>,
    dir: &str,
    src: &str,
    alt: &str,
    place: Option<&str>,
    repeated: &HashSet<u64>,
) -> Embed {
    match chapter_bytes(archive, dir, src) {
        Some(bytes) => images::embed(store, &bytes, alt, place, repeated),
        None => Embed::Fallback,
    }
}

/// Pictures that appear in three or more chapters: ornaments and publisher marks.
fn repeated_pictures(archive: &SharedArchive<'_>, chapters: &[Chapter]) -> HashSet<u64> {
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for chapter in chapters.iter().filter(|chapter| !chapter.navigation) {
        // Including an authored TOC must not turn illustrations in existing
        // chapters into repeated ornaments.
        let path = &chapter.path;
        let result = read_entry(&mut archive.borrow_mut(), path);
        let xhtml = match result {
            Ok(xhtml) => xhtml,
            Err(_) => {
                if archive.borrow().limits.check().is_err() {
                    break;
                }
                continue;
            }
        };
        let dir = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let found = RefCell::new(HashSet::new());
        let collect = |src: &str, _alt: &str| {
            if let Some(bytes) = chapter_bytes(archive, dir, src) {
                found.borrow_mut().insert(images::content_key(&bytes));
            }
            Embed::Skip
        };
        let collect: &dyn Fn(&str, &str) -> Embed = &collect;
        let render = chapter.render_options(Some(collect));
        // Prepass reads share the same cumulative container accounting.
        if render_html(&xhtml, &render).is_err() {
            break;
        }
        if archive.borrow().limits.check().is_err() {
            break;
        }
        for key in found.into_inner() {
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, chapters)| *chapters >= images::REPEAT_UNITS)
        .map(|(key, _)| key)
        .collect()
}
