//! EPUB → Markdown: container.xml → OPF → spine order, one section per chapter.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader;
use zip::ZipArchive;

use crate::html::{render_html, RenderOptions};
use crate::images::{self, Embed, ImageStore};
use crate::{ConvertError, Converted, Options, Section};

const MAX_ENTRIES: usize = 10_000;
const MAX_TOTAL_UNCOMPRESSED: u64 = 200 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 50 * 1024 * 1024;

pub fn convert(bytes: &[u8], options: &Options) -> Result<Converted, ConvertError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ConvertError::Invalid(format!("not a valid EPUB zip: {e}")))?;
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

    let opf_path = match read_entry(&mut archive, "META-INF/container.xml") {
        Ok(container) => rootfile_path(&container),
        Err(_) => None,
    }
    .or_else(|| {
        archive
            .file_names()
            .find(|n| n.to_ascii_lowercase().ends_with(".opf"))
            .map(str::to_string)
    })
    .ok_or_else(|| ConvertError::Invalid("EPUB has no package document (OPF)".into()))?;
    let opf = read_entry(&mut archive, &opf_path)?;
    let package = parse_opf(&opf)?;
    let opf_dir = opf_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");

    // Chapters in reading order: the archive path of each and its XHTML.
    let mut chapters: Vec<(String, String)> = Vec::new();
    for idref in &package.spine {
        let Some(item) = package.manifest.get(idref) else {
            continue;
        };
        if item.properties.split_whitespace().any(|p| p == "nav")
            || !(item.media_type.contains("html") || item.media_type.is_empty())
        {
            continue;
        }
        let path = join_path(
            opf_dir,
            &percent_decode(item.href.split('#').next().unwrap_or("")),
        );
        let Ok(xhtml) = read_entry(&mut archive, &path) else {
            continue;
        };
        chapters.push((path, xhtml));
    }

    let store = options.images.as_ref();
    let archive = RefCell::new(archive);
    // Pictures on three or more chapters (ornaments, publisher marks) are left out.
    let repeated = match store {
        Some(_) => repeated_pictures(&archive, &chapters),
        None => HashSet::new(),
    };
    let mut sections = Vec::new();
    let mut section_paths = Vec::new();
    for (path, xhtml) in &chapters {
        let number = sections.len() + 1;
        let place = format!("chapter {number}");
        let dir = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let decide = |src: &str, alt: &str| match store {
            Some(store) => chapter_image(store, &archive, dir, src, alt, Some(&place), &repeated),
            None => Embed::Fallback,
        };
        let decide: &dyn Fn(&str, &str) -> Embed = &decide;
        let render = RenderOptions {
            base: None,
            keep_relative_links: false,
            select_main: false,
            images: store.map(|_| decide),
        };
        let markdown = render_html(xhtml, &render);
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

/// EPUB 3 navigation documents and EPUB 2 NCX share the same chapter ranges.
fn native_outline(
    archive: &SharedArchive<'_>,
    package: &Package,
    opf_dir: &str,
    paths: &[String],
) -> Vec<(usize, String, Option<u32>)> {
    let mut items: Vec<_> = package
        .manifest
        .values()
        .filter(|item| {
            item.properties.split_whitespace().any(|p| p == "nav")
                || item.media_type == "application/x-dtbncx+xml"
        })
        .collect();
    items.sort_by_key(|item| {
        (
            !item.properties.split_whitespace().any(|p| p == "nav"),
            item.href.clone(),
        )
    });
    for item in items {
        let path = join_path(opf_dir, &percent_decode(&item.href));
        let Ok(xml) = read_entry(&mut archive.borrow_mut(), &path) else {
            continue;
        };
        let base = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let html = scraper::Html::parse_document(&xml);
        let selector = scraper::Selector::parse("nav").unwrap();
        let link_selector = scraper::Selector::parse("a[href]").unwrap();
        let item_selector = scraper::Selector::parse("li").unwrap();
        let mut found = Vec::new();
        for nav in html.select(&selector) {
            let kind = nav
                .value()
                .attr("epub:type")
                .or_else(|| nav.value().attr("type"));
            if kind.is_some_and(|kind| !kind.split_whitespace().any(|k| k == "toc")) {
                continue;
            }
            for item in nav.select(&item_selector) {
                let label = item
                    .children()
                    .filter_map(scraper::ElementRef::wrap)
                    .find(|e| matches!(e.value().name(), "a" | "span"));
                let Some(link) = label.or_else(|| item.select(&link_selector).next()) else {
                    continue;
                };
                let destination = link.value().attr("href").map(str::to_string).or_else(|| {
                    item.select(&link_selector)
                        .find_map(|e| e.value().attr("href").map(str::to_string))
                });
                let Some(href) = destination else {
                    continue;
                };
                let target = join_path(base, &percent_decode(href.split('#').next().unwrap_or("")));
                let Some(index) = paths.iter().position(|p| p == &target) else {
                    continue;
                };
                let title = link
                    .text()
                    .collect::<Vec<_>>()
                    .join("")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let level = link
                    .ancestors()
                    .filter_map(scraper::ElementRef::wrap)
                    .take_while(|e| e.value().name() != "nav")
                    .filter(|e| e.value().name() == "li")
                    .count()
                    .saturating_sub(1);
                if !title.is_empty() {
                    found.push((level, title, Some(index as u32 + 1)));
                }
            }
        }
        if found.is_empty() {
            // NCX is XML; quick-xml preserves navPoint case and nesting.
            let mut reader = Reader::from_str(&xml);
            let mut depth = 0usize;
            let mut pending: Vec<(usize, String, Option<u32>)> = Vec::new();
            let mut points = Vec::new();
            let mut capture = false;
            while let Ok(event) = reader.read_event() {
                match event {
                    Event::Start(e) if e.local_name().as_ref() == "navPoint" => {
                        points.push(pending.len());
                        pending.push((depth, String::new(), None));
                        depth += 1;
                    }
                    Event::Start(e) if e.local_name().as_ref() == "text" => capture = true,
                    Event::Text(t) if capture => {
                        if let Some(&index) = points.last() {
                            pending[index]
                                .1
                                .push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
                        }
                    }
                    Event::CData(t) if capture => {
                        if let Some(&index) = points.last() {
                            pending[index]
                                .1
                                .push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
                        }
                    }
                    Event::GeneralRef(r) if capture => {
                        if let Some(&index) = points.last() {
                            if let Ok(Some(ch)) = r.resolve_char_ref() {
                                pending[index].1.push(ch);
                            } else if let Some(text) =
                                quick_xml::escape::resolve_predefined_entity(&r)
                            {
                                pending[index].1.push_str(text);
                            }
                        }
                    }
                    Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == "content" => {
                        let src = attr(&e, "src");
                        if let (Some(&index), Some(src)) = (points.last(), src) {
                            let target = join_path(
                                base,
                                &percent_decode(src.split('#').next().unwrap_or("")),
                            );
                            pending[index].2 = paths
                                .iter()
                                .position(|p| p == &target)
                                .map(|p| p as u32 + 1);
                        }
                    }
                    Event::End(e) if e.local_name().as_ref() == "navPoint" => {
                        points.pop();
                        depth = depth.saturating_sub(1);
                    }
                    Event::End(e) if e.local_name().as_ref() == "text" => capture = false,
                    Event::Eof => break,
                    _ => {}
                }
            }
            found = pending
                .into_iter()
                .filter(|(_, title, _)| !title.trim().is_empty())
                .map(|(level, title, page)| {
                    (
                        level,
                        title.split_whitespace().collect::<Vec<_>>().join(" "),
                        page,
                    )
                })
                .collect();
        }
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String, ConvertError> {
    let buf = read_entry_bytes(archive, name)?;
    let buf = buf.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&buf);
    Ok(String::from_utf8_lossy(buf).into_owned())
}

fn read_entry_bytes(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, ConvertError> {
    let resolved = if archive.index_for_name(name).is_some() {
        name.to_string()
    } else {
        // Some producers get case wrong; fall back to a case-insensitive match.
        archive
            .file_names()
            .find(|n| n.eq_ignore_ascii_case(name))
            .map(str::to_string)
            .ok_or_else(|| ConvertError::Invalid(format!("EPUB entry {name} is missing")))?
    };
    let entry = archive
        .by_name(&resolved)
        .map_err(|e| ConvertError::Invalid(format!("EPUB entry {name}: {e}")))?;
    let mut buf = Vec::new();
    entry
        .take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| ConvertError::Invalid(format!("EPUB entry {name}: {e}")))?;
    if buf.len() as u64 > MAX_ENTRY_BYTES {
        return Err(ConvertError::Invalid(format!(
            "EPUB entry {name} is too large"
        )));
    }
    Ok(buf)
}

type SharedArchive<'a> = RefCell<ZipArchive<Cursor<&'a [u8]>>>;

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
fn repeated_pictures(archive: &SharedArchive<'_>, chapters: &[(String, String)]) -> HashSet<u64> {
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for (path, xhtml) in chapters {
        let dir = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let found = RefCell::new(HashSet::new());
        let collect = |src: &str, _alt: &str| {
            if let Some(bytes) = chapter_bytes(archive, dir, src) {
                found.borrow_mut().insert(images::content_key(&bytes));
            }
            Embed::Skip
        };
        let collect: &dyn Fn(&str, &str) -> Embed = &collect;
        let render = RenderOptions {
            base: None,
            keep_relative_links: false,
            select_main: false,
            images: Some(collect),
        };
        render_html(xhtml, &render);
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

fn rootfile_path(container: &str) -> Option<String> {
    let mut reader = Reader::from_str(container);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == "rootfile" => {
                if let Some(path) = attr(&e, "full-path") {
                    return Some(path);
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

struct ManifestItem {
    href: String,
    media_type: String,
    properties: String,
}

#[derive(Default)]
struct Package {
    title: Option<String>,
    creators: Vec<String>,
    publisher: Option<String>,
    date: Option<String>,
    language: Option<String>,
    manifest: HashMap<String, ManifestItem>,
    spine: Vec<String>,
}

fn parse_opf(opf: &str) -> Result<Package, ConvertError> {
    let mut reader = Reader::from_str(opf);
    let mut package = Package::default();
    let mut capture: Option<&'static str> = None;
    let mut text = String::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|e| ConvertError::Invalid(format!("malformed OPF: {e}")))?;
        match event {
            Event::Start(e) => {
                capture = match e.local_name().as_ref() {
                    "title" => Some("title"),
                    "creator" => Some("creator"),
                    "publisher" => Some("publisher"),
                    "date" => Some("date"),
                    "language" => Some("language"),
                    _ => None,
                };
                text.clear();
                opf_element(&e, &mut package);
            }
            Event::Empty(e) => opf_element(&e, &mut package),
            Event::Text(t) if capture.is_some() => {
                text.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0))
            }
            Event::CData(t) if capture.is_some() => {
                text.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0))
            }
            Event::GeneralRef(r) if capture.is_some() => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(s) = quick_xml::escape::resolve_predefined_entity(&r) {
                    text.push_str(s);
                }
            }
            Event::End(_) => {
                if let Some(field) = capture.take() {
                    let value = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !value.is_empty() {
                        match field {
                            "title" => package.title = package.title.take().or(Some(value)),
                            "creator" => package.creators.push(value),
                            "publisher" => {
                                package.publisher = package.publisher.take().or(Some(value))
                            }
                            "date" => package.date = package.date.take().or(Some(value)),
                            _ => package.language = package.language.take().or(Some(value)),
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(package)
}

fn opf_element(e: &quick_xml::events::BytesStart<'_>, package: &mut Package) {
    match e.local_name().as_ref() {
        "item" => {
            if let (Some(id), Some(href)) = (attr(e, "id"), attr(e, "href")) {
                package.manifest.insert(
                    id,
                    ManifestItem {
                        href,
                        media_type: attr(e, "media-type").unwrap_or_default(),
                        properties: attr(e, "properties").unwrap_or_default(),
                    },
                );
            }
        }
        "itemref" => {
            if let Some(idref) = attr(e, "idref") {
                package.spine.push(idref);
            }
        }
        _ => {}
    }
}

fn attr(e: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        })
}

/// Resolve `href` against the OPF directory, normalising `.` and `..`.
fn join_path(dir: &str, href: &str) -> String {
    let mut parts: Vec<&str> = if href.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|p| !p.is_empty()).collect()
    };
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn has_text(markdown: &str) -> bool {
    markdown.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !(line.starts_with("![") && line.ends_with(')')) && line != "---"
    })
}

fn first_heading(markdown: &str) -> Option<String> {
    let line = markdown.lines().find(|l| l.starts_with('#'))?;
    let heading = line.trim_start_matches('#').trim();
    // Keep labels short: one line, bounded length.
    let heading: String = heading.chars().take(80).collect();
    (!heading.is_empty()).then_some(heading)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    pub(crate) fn build_epub(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut out);
            let stored =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("mimetype", stored).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();
            for (name, body) in files {
                zip.start_file(*name, deflated).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        out.into_inner()
    }

    const CONTAINER: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

    const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Tales &amp; Roads</dc:title>
    <dc:creator>Ann Author</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="cover" href="text/cover.xhtml" media-type="application/xhtml+xml"/>
    <item id="c1" href="text/chapter%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/c2.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine><itemref idref="nav"/><itemref idref="cover"/><itemref idref="c2"/><itemref idref="c1"/><itemref idref="missing"/></spine>
</package>"#;

    #[test]
    fn follows_spine_order_and_labels_chapters() {
        let epub = build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", OPF),
            ("OEBPS/nav.xhtml", "<html><body><nav><ol><li><a href='text/c2.xhtml'>Two</a></li></ol></nav></body></html>"),
            ("OEBPS/text/cover.xhtml", "<html><body><img src='../img/cover.jpg' alt='Cover'/></body></html>"),
            ("OEBPS/text/chapter 1.xhtml", "<html><head><title>Book</title></head><body><h1>The Road</h1><p>It was <em>long</em>. See <a href='c2.xhtml#n1'>note</a>.</p></body></html>"),
            ("OEBPS/text/c2.xhtml", "<html><body><h2>Before</h2><p>First &amp; foremost.</p></body></html>"),
        ]);
        let converted = convert(&epub, &Options::default()).unwrap();
        assert_eq!(converted.format, "epub");
        assert_eq!(converted.outline, vec![(0, "Two".into(), Some(1))]);
        assert_eq!(converted.title.as_deref(), Some("Tales & Roads"));
        assert_eq!(
            converted.metadata[0],
            ("author".into(), "Ann Author".into())
        );
        let labels: Vec<&str> = converted
            .sections
            .iter()
            .map(|s| s.label.as_str())
            .collect();
        assert_eq!(labels, ["chapter 1: Before", "chapter 2: The Road"]);
        assert_eq!(
            converted.sections[0].markdown,
            "## Before\n\nFirst & foremost."
        );
        assert_eq!(
            converted.sections[1].markdown,
            "# The Road\n\nIt was *long*. See note."
        );
    }

    #[test]
    fn native_tocs_preserve_nesting_and_prefer_toc_over_landmarks() {
        let nav = "<html><body><nav epub:type='landmarks'><ol><li><a href='text/c2.xhtml'>Skip</a></li></ol></nav><nav epub:type='toc'><ol><li><span>Part</span><ol><li><a href='text/c2.xhtml'>Before &amp; after</a><ol><li><a href='text/chapter%201.xhtml#road'>Road</a></li></ol></li></ol></li></ol></nav></body></html>";
        let epub = build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", OPF),
            ("OEBPS/nav.xhtml", nav),
            ("OEBPS/text/c2.xhtml", "<h1>Before</h1><p>First.</p>"),
            (
                "OEBPS/text/chapter 1.xhtml",
                "<h1>The Road</h1><p>Second.</p>",
            ),
        ]);
        let converted = convert(&epub, &Options::default()).unwrap();
        assert_eq!(
            converted.outline,
            vec![
                (0, "Part".into(), Some(1)),
                (1, "Before & after".into(), Some(1)),
                (2, "Road".into(), Some(2))
            ]
        );
    }

    #[test]
    fn reads_epub2_ncx_without_changing_chapter_markdown() {
        let opf = OPF.replace(
            "href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"",
            "href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"",
        );
        let ncx = "<ncx><navMap><navPoint><navLabel><text>Before &amp; after</text></navLabel><content src='text/c2.xhtml'/><navPoint><navLabel><text><![CDATA[Road]]></text></navLabel><content src='text/chapter%201.xhtml#road'/></navPoint></navPoint></navMap></ncx>";
        let epub = build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", &opf),
            ("OEBPS/toc.ncx", ncx),
            ("OEBPS/text/c2.xhtml", "<h1>Before</h1><p>First.</p>"),
            (
                "OEBPS/text/chapter 1.xhtml",
                "<h1>The Road</h1><p>Second.</p>",
            ),
        ]);
        let converted = convert(&epub, &Options::default()).unwrap();
        assert_eq!(
            converted.outline,
            vec![
                (0, "Before & after".into(), Some(1)),
                (1, "Road".into(), Some(2))
            ]
        );
        assert_eq!(converted.sections[0].markdown, "# Before\n\nFirst.");
    }

    #[test]
    fn rejects_garbage_and_bounds() {
        assert!(matches!(
            convert(b"not a zip", &Options::default()),
            Err(ConvertError::Invalid(_))
        ));
        let no_opf = build_epub(&[("hello.txt", "hi")]);
        assert!(matches!(
            convert(&no_opf, &Options::default()),
            Err(ConvertError::Invalid(_))
        ));
        let bad_opf = build_epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", "<package><spine>"),
        ]);
        assert!(convert(&bad_opf, &Options::default()).is_err());
    }

    #[test]
    fn path_helpers() {
        assert_eq!(join_path("OEBPS", "../img/a.png"), "img/a.png");
        assert_eq!(join_path("", "text/./a.xhtml"), "text/a.xhtml");
        assert_eq!(percent_decode("chapter%201%2"), "chapter 1%2");
        assert_eq!(percent_decode("%é%"), "%é%");
        assert_eq!(percent_decode("%E4%B8%AD"), "中");
    }
}
