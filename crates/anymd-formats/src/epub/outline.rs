//! EPUB 3 navigation and EPUB 2 NCX outlines mapped to rendered chapters.

use quick_xml::events::Event;
use quick_xml::Reader;

use super::package::{attr, join_path, percent_decode, read_entry, Package, SharedArchive};

/// EPUB 3 navigation documents and EPUB 2 NCX share the same chapter ranges.
/// Most table-of-contents entries and nesting levels read from an EPUB.
const MAX_TOC_ENTRIES: usize = 10_000;
const MAX_TOC_DEPTH: usize = 64;

pub(super) fn native_outline(
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
            // A hostile nav can nest thousands of lists; the tree stays useful and
            // linear-time under these caps.
            for item in nav.select(&item_selector).take(MAX_TOC_ENTRIES) {
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
                    .take(MAX_TOC_DEPTH + 1)
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
                        if pending.len() >= MAX_TOC_ENTRIES {
                            break;
                        }
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

pub(super) fn has_text(markdown: &str) -> bool {
    markdown.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !(line.starts_with("![") && line.ends_with(')')) && line != "---"
    })
}

pub(super) fn first_heading(markdown: &str) -> Option<String> {
    let line = markdown.lines().find(|l| l.starts_with('#'))?;
    let heading = line.trim_start_matches('#').trim();
    // Keep labels short: one line, bounded length.
    let heading: String = heading.chars().take(80).collect();
    (!heading.is_empty()).then_some(heading)
}
