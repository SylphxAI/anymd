//! EPUB → Markdown: container.xml → OPF → spine order, one section per chapter.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader;
use zip::ZipArchive;

use crate::html::{render_html, RenderOptions};
use crate::{ConvertError, Converted, Options, Section};

const MAX_ENTRIES: usize = 10_000;
const MAX_TOTAL_UNCOMPRESSED: u64 = 200 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 50 * 1024 * 1024;

pub fn convert(bytes: &[u8], _options: &Options) -> Result<Converted, ConvertError> {
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

    let render = RenderOptions {
        base: None,
        keep_relative_links: false,
        select_main: false,
    };
    let mut sections = Vec::new();
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
        let markdown = render_html(&xhtml, &render);
        if !has_text(&markdown) {
            continue;
        }
        let number = sections.len() + 1;
        let label = match first_heading(&markdown) {
            Some(heading) => format!("chapter {number}: {heading}"),
            None => format!("chapter {number}"),
        };
        sections.push(Section { label, markdown });
    }
    if sections.is_empty() && package.spine.is_empty() {
        return Err(ConvertError::Invalid("EPUB spine is empty".into()));
    }

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
        format: "epub".into(),
        title: package.title,
        sections,
        metadata,
    })
}

fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String, ConvertError> {
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
    let buf = buf.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&buf);
    Ok(String::from_utf8_lossy(buf).into_owned())
}

fn rootfile_path(container: &str) -> Option<String> {
    let mut reader = Reader::from_str(container);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == b"rootfile" => {
                if let Some(path) = attr(&e, b"full-path") {
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
                    b"title" => Some("title"),
                    b"creator" => Some("creator"),
                    b"publisher" => Some("publisher"),
                    b"date" => Some("date"),
                    b"language" => Some("language"),
                    _ => None,
                };
                text.clear();
                opf_element(&e, &mut package);
            }
            Event::Empty(e) => opf_element(&e, &mut package),
            Event::Text(t) if capture.is_some() => text.push_str(&t.decode().unwrap_or_default()),
            Event::CData(t) if capture.is_some() => text.push_str(&t.decode().unwrap_or_default()),
            Event::GeneralRef(r) if capture.is_some() => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(s) = r
                    .decode()
                    .ok()
                    .and_then(|name| quick_xml::escape::resolve_predefined_entity(&name))
                {
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
        b"item" => {
            if let (Some(id), Some(href)) = (attr(e, b"id"), attr(e, b"href")) {
                package.manifest.insert(
                    id,
                    ManifestItem {
                        href,
                        media_type: attr(e, b"media-type").unwrap_or_default(),
                        properties: attr(e, b"properties").unwrap_or_default(),
                    },
                );
            }
        }
        b"itemref" => {
            if let Some(idref) = attr(e, b"idref") {
                package.spine.push(idref);
            }
        }
        _ => {}
    }
}

fn attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| {
            a.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, e.decoder())
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
