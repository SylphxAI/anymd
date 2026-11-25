//! EPUB archive entry reads and OPF package parsing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader;
use zip::ZipArchive;

use super::MAX_ENTRY_BYTES;
use crate::limits::Limits;
use crate::ConvertError;

pub(super) struct Archive<'a> {
    pub(super) zip: ZipArchive<Cursor<&'a [u8]>>,
    pub(super) limits: Limits,
}

pub(super) type SharedArchive<'a> = RefCell<Archive<'a>>;

pub(super) fn read_entry(archive: &mut Archive<'_>, name: &str) -> Result<String, ConvertError> {
    let buf = read_entry_bytes(archive, name)?;
    let buf = buf.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&buf);
    Ok(String::from_utf8_lossy(buf).into_owned())
}

pub(super) fn read_entry_bytes(
    archive: &mut Archive<'_>,
    name: &str,
) -> Result<Vec<u8>, ConvertError> {
    archive.limits.check()?;
    let resolved = if archive.zip.index_for_name(name).is_some() {
        name.to_string()
    } else {
        // Some producers get case wrong; fall back to a case-insensitive match.
        archive
            .zip
            .file_names()
            .filter_map(|name| name.ok())
            .find(|n| n.eq_ignore_ascii_case(name))
            .map(|name| name.to_string())
            .ok_or_else(|| ConvertError::Invalid(format!("EPUB entry {name} is missing")))?
    };
    let entry = archive
        .zip
        .by_name(&resolved)
        .map_err(|e| ConvertError::Invalid(format!("EPUB entry {name}: {e}")))?;
    let buf = archive.limits.read(entry.take(MAX_ENTRY_BYTES + 1))?;
    if buf.len() as u64 > MAX_ENTRY_BYTES {
        return Err(ConvertError::Invalid(format!(
            "EPUB entry {name} is too large"
        )));
    }
    Ok(buf)
}

pub(super) fn rootfile_path(container: &str) -> Option<String> {
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

pub(super) struct ManifestItem {
    pub(super) href: String,
    pub(super) media_type: String,
    pub(super) properties: String,
}

#[derive(Default)]
pub(super) struct Package {
    pub(super) title: Option<String>,
    pub(super) creators: Vec<String>,
    pub(super) publisher: Option<String>,
    pub(super) date: Option<String>,
    pub(super) language: Option<String>,
    pub(super) manifest: HashMap<String, ManifestItem>,
    pub(super) spine: Vec<String>,
}

pub(super) fn parse_opf(opf: &str) -> Result<Package, ConvertError> {
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

pub(super) fn attr(e: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
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
pub(super) fn join_path(dir: &str, href: &str) -> String {
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

pub(super) fn percent_decode(value: &str) -> String {
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
