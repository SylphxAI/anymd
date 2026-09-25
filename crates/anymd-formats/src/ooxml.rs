//! Shared plumbing for the Office Open XML converters: a bounded zip reader,
//! a tiny XML tree, relationship resolution, core properties, and the inline
//! run renderer that turns formatted text into compact Markdown.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::ConvertError;

/// Largest uncompressed zip entry we will inflate.
pub(crate) const MAX_ENTRY_BYTES: u64 = 200 * 1024 * 1024;
/// Largest number of entries a container may declare.
pub(crate) const MAX_ENTRIES: usize = 10_000;
/// Deepest XML nesting we accept; deeper input is treated as hostile.
const MAX_DEPTH: usize = 512;

pub(crate) fn invalid(message: impl std::fmt::Display) -> ConvertError {
    ConvertError::Invalid(message.to_string())
}

/// A zip container with entry-count and inflated-size limits enforced.
pub(crate) struct Package<'a> {
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
    names: Vec<String>,
}

impl<'a> Package<'a> {
    pub(crate) fn open(bytes: &'a [u8], kind: &str) -> Result<Self, ConvertError> {
        let archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| invalid(format!("not a valid {kind} file: {e}")))?;
        check_limits(&archive)?;
        let names = archive.file_names().map(str::to_string).collect();
        Ok(Self { archive, names })
    }

    fn resolve_name(&self, name: &str) -> Option<String> {
        let name = name.trim_start_matches('/');
        if self.names.iter().any(|n| n == name) {
            return Some(name.to_string());
        }
        self.names
            .iter()
            .find(|n| n.eq_ignore_ascii_case(name))
            .cloned()
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.resolve_name(name).is_some()
    }

    /// Read one entry, or `None` when it does not exist.
    pub(crate) fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, ConvertError> {
        let Some(name) = self.resolve_name(name) else {
            return Ok(None);
        };
        let file = self
            .archive
            .by_name(&name)
            .map_err(|e| invalid(format!("cannot read {name}: {e}")))?;
        if file.size() > MAX_ENTRY_BYTES {
            return Err(invalid(format!(
                "{name} is larger than {MAX_ENTRY_BYTES} bytes uncompressed"
            )));
        }
        let mut out = Vec::with_capacity(file.size().min(16 * 1024 * 1024) as usize);
        // The header size can lie; bound the actual inflated stream too.
        file.take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut out)
            .map_err(|e| invalid(format!("cannot inflate {name}: {e}")))?;
        if out.len() as u64 > MAX_ENTRY_BYTES {
            return Err(invalid(format!(
                "{name} is larger than {MAX_ENTRY_BYTES} bytes uncompressed"
            )));
        }
        Ok(Some(out))
    }

    /// Read and parse an XML entry, or `None` when it does not exist.
    pub(crate) fn xml(&mut self, name: &str) -> Result<Option<Element>, ConvertError> {
        match self.read(name)? {
            Some(bytes) => parse_xml(&bytes)
                .map(Some)
                .map_err(|e| invalid(format!("{name}: {e}"))),
            None => Ok(None),
        }
    }

    /// Relationships of a part (`dir/_rels/file.rels`), keyed by id.
    pub(crate) fn rels(&mut self, part: &str) -> Result<Rels, ConvertError> {
        let (dir, file) = split_part(part);
        let rels_name = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let mut rels = Rels::default();
        let Some(root) = self.xml(&rels_name)? else {
            return Ok(rels);
        };
        for rel in root.children_named("Relationship") {
            let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) else {
                continue;
            };
            let external = rel
                .attr("TargetMode")
                .is_some_and(|m| m.eq_ignore_ascii_case("External"));
            let target = if external {
                target.to_string()
            } else {
                resolve_target(dir, target)
            };
            rels.by_id.insert(
                id.to_string(),
                Rel {
                    kind: rel.attr("Type").unwrap_or_default().to_string(),
                    target,
                    external,
                },
            );
            rels.order.push(id.to_string());
        }
        Ok(rels)
    }

    /// The main part named by the package-level officeDocument relationship.
    pub(crate) fn main_part(&mut self, fallback: &str) -> Result<String, ConvertError> {
        let rels = self.rels("")?;
        let found = rels
            .first_of_type("/officeDocument")
            .map(|r| r.target.clone());
        Ok(found
            .filter(|t| self.has(t))
            .unwrap_or_else(|| fallback.to_string()))
    }
}

/// Reject containers that declare too many entries or oversized members.
pub(crate) fn check_limits<R: Read + std::io::Seek>(
    archive: &zip::ZipArchive<R>,
) -> Result<(), ConvertError> {
    if archive.len() > MAX_ENTRIES {
        return Err(invalid(format!(
            "container has {} entries (limit {MAX_ENTRIES})",
            archive.len()
        )));
    }
    Ok(())
}

/// Validate a zip-based input (entry count and declared sizes) without keeping it open.
pub(crate) fn guard_zip(bytes: &[u8]) -> Result<(), ConvertError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| invalid(format!("not a valid zip container: {e}")))?;
    check_limits(&archive)?;
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|e| invalid(format!("bad zip entry: {e}")))?;
        if file.size() > MAX_ENTRY_BYTES {
            return Err(invalid(format!(
                "{} is larger than {MAX_ENTRY_BYTES} bytes uncompressed",
                file.name()
            )));
        }
    }
    Ok(())
}

fn split_part(part: &str) -> (&str, &str) {
    let part = part.trim_start_matches('/');
    match part.rfind('/') {
        Some(i) => (&part[..i], &part[i + 1..]),
        None => ("", part),
    }
}

/// Resolve a relationship target relative to the source part's directory.
fn resolve_target(dir: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or_default();
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[derive(Debug, Clone)]
pub(crate) struct Rel {
    pub kind: String,
    pub target: String,
    pub external: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Rels {
    by_id: HashMap<String, Rel>,
    order: Vec<String>,
}

impl Rels {
    pub(crate) fn get(&self, id: &str) -> Option<&Rel> {
        self.by_id.get(id)
    }

    pub(crate) fn first_of_type(&self, suffix: &str) -> Option<&Rel> {
        self.order
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .find(|r| r.kind.ends_with(suffix))
    }

    /// A hyperlink target for a relationship id: external URLs only.
    pub(crate) fn link(&self, id: &str) -> Option<String> {
        self.get(id)
            .filter(|r| r.external && !r.target.is_empty())
            .map(|r| r.target.clone())
    }
}

/// A parsed XML element. Names keep their prefix; lookups match local names.
#[derive(Debug, Default, Clone)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}

pub(crate) fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl Element {
    pub(crate) fn local(&self) -> &str {
        local(&self.name)
    }

    pub(crate) fn is(&self, name: &str) -> bool {
        self.local() == name
    }

    /// Attribute by local name (any prefix).
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| local(k) == name)
            .map(|(_, v)| v.as_str())
    }

    /// A relationship reference (`r:id`, `r:embed`): a prefixed attribute with this local name.
    pub(crate) fn rel_attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.contains(':') && local(k) == name)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub(crate) fn children_named<'s>(
        &'s self,
        name: &'s str,
    ) -> impl Iterator<Item = &'s Element> + 's {
        self.elements().filter(move |e| e.is(name))
    }

    pub(crate) fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.is(name))
    }

    /// Follow a path of local names through first-matching children.
    pub(crate) fn path(&self, names: &[&str]) -> Option<&Element> {
        names.iter().try_fold(self, |node, name| node.child(name))
    }

    /// First descendant (depth-first, including self) with this local name.
    pub(crate) fn find(&self, name: &str) -> Option<&Element> {
        if self.is(name) {
            return Some(self);
        }
        self.elements().find_map(|e| e.find(name))
    }

    /// All descendants with this local name, not descending into matches.
    pub(crate) fn find_all<'s>(&'s self, name: &str, out: &mut Vec<&'s Element>) {
        for e in self.elements() {
            if e.is(name) {
                out.push(e);
            } else {
                e.find_all(name, out);
            }
        }
    }

    /// Concatenated character data of this subtree.
    pub(crate) fn text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for child in &self.children {
            match child {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => e.collect_text(out),
            }
        }
    }

    /// Whether a toggle property element (`<w:b/>`, `<w:b w:val="0"/>`) is on.
    pub(crate) fn toggle(&self, name: &str) -> Option<bool> {
        self.child(name)
            .map(|e| !matches!(e.attr("val"), Some("0" | "false" | "off" | "none")))
    }
}

/// Parse XML bytes into an element tree (the document element).
pub(crate) fn parse_xml(bytes: &[u8]) -> Result<Element, String> {
    let text = decode(bytes);
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Element> = vec![Element::default()];
    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("malformed XML at byte {}: {e}", reader.buffer_position()))?;
        match event {
            Event::Start(start) => {
                if stack.len() > MAX_DEPTH {
                    return Err("XML nesting too deep".into());
                }
                stack.push(element_from(&start)?);
            }
            Event::Empty(start) => {
                let element = element_from(&start)?;
                push_child(&mut stack, Node::Element(element));
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    return Err("unbalanced end tag".into());
                }
                let element = stack.pop().unwrap_or_default();
                push_child(&mut stack, Node::Element(element));
            }
            Event::Text(t) => {
                let s = t.decode().map_err(|e| e.to_string())?;
                push_text(&mut stack, &s);
            }
            Event::CData(t) => {
                let s = t.decode().map_err(|e| e.to_string())?;
                push_text(&mut stack, &s);
            }
            Event::GeneralRef(r) => {
                let resolved = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => {
                        let name = r.decode().map_err(|e| e.to_string())?;
                        match name.as_ref() {
                            "amp" => "&".into(),
                            "lt" => "<".into(),
                            "gt" => ">".into(),
                            "quot" => "\"".into(),
                            "apos" => "'".into(),
                            other => format!("&{other};"),
                        }
                    }
                };
                push_text(&mut stack, &resolved);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    // Close anything left open (truncated input) rather than failing outright.
    while stack.len() > 1 {
        let element = stack.pop().unwrap_or_default();
        push_child(&mut stack, Node::Element(element));
    }
    let root = stack.pop().unwrap_or_default();
    root.children
        .into_iter()
        .find_map(|c| match c {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
        .ok_or_else(|| "no document element".to_string())
}

fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn element_from(start: &quick_xml::events::BytesStart<'_>) -> Result<Element, String> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for attr in start.attributes().with_checks(false) {
        let attr = attr.map_err(|e| e.to_string())?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        let value = match attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
            Ok(v) => v.into_owned(),
            Err(_) => String::from_utf8_lossy(&attr.value).into_owned(),
        };
        attrs.push((key, value));
    }
    Ok(Element {
        name,
        attrs,
        children: Vec::new(),
    })
}

fn push_child(stack: &mut [Element], node: Node) {
    if let Some(top) = stack.last_mut() {
        top.children.push(node);
    }
}

fn push_text(stack: &mut [Element], text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(top) = stack.last_mut() {
        if let Some(Node::Text(existing)) = top.children.last_mut() {
            existing.push_str(text);
        } else {
            top.children.push(Node::Text(text.to_string()));
        }
    }
}

/// Title and small metadata from `docProps/core.xml`.
pub(crate) fn core_properties(
    package: &mut Package<'_>,
) -> (Option<String>, Vec<(String, String)>) {
    let Ok(Some(core)) = package.xml("docProps/core.xml") else {
        return (None, Vec::new());
    };
    let field = |name: &str| {
        core.child(name)
            .map(|e| collapse_ws(&e.text()))
            .filter(|s| !s.is_empty())
    };
    let title = field("title");
    let mut metadata = Vec::new();
    if let Some(author) = field("creator") {
        metadata.push(("author".to_string(), author));
    }
    for (key, name) in [
        ("subject", "subject"),
        ("created", "created"),
        ("modified", "modified"),
    ] {
        if let Some(value) = field(name) {
            metadata.push((key.to_string(), value));
        }
    }
    (title, metadata)
}

pub(crate) fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One formatted stretch of inline text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub link: Option<String>,
}

/// Accumulates spans for one paragraph.
#[derive(Debug, Default)]
pub(crate) struct Inline {
    spans: Vec<Span>,
}

impl Inline {
    pub(crate) fn push(&mut self, text: &str, bold: bool, italic: bool, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if last.bold == bold && last.italic == italic && last.link.as_deref() == link {
                last.text.push_str(text);
                return;
            }
        }
        self.spans.push(Span {
            text: text.to_string(),
            bold,
            italic,
            link: link.map(str::to_string),
        });
    }

    pub(crate) fn is_blank(&self) -> bool {
        self.spans.iter().all(|s| s.text.trim().is_empty())
    }

    /// Render to Markdown. `emphasis` false drops bold/italic (headings).
    pub(crate) fn render(&self, emphasis: bool) -> String {
        let mut out = String::new();
        let mut index = 0;
        while index < self.spans.len() {
            let link = self.spans[index].link.clone();
            let mut end = index + 1;
            while end < self.spans.len() && self.spans[end].link == link {
                end += 1;
            }
            let group = &self.spans[index..end];
            match link {
                Some(url) if !group.iter().all(|s| s.text.trim().is_empty()) => {
                    let inner = render_emphasis(group, emphasis);
                    let (lead, core, trail) = split_ws(&inner);
                    let label = core.replace('[', "\\[").replace(']', "\\]");
                    let url = url.replace(' ', "%20").replace(')', "%29");
                    if label == url
                        || (label.contains('@')
                            && label.trim_start_matches("mailto:")
                                == url.trim_start_matches("mailto:"))
                    {
                        out.push_str(&format!("{lead}<{url}>{trail}"));
                    } else {
                        out.push_str(&format!("{lead}[{label}]({url}){trail}"));
                    }
                }
                _ => out.push_str(&render_emphasis(group, emphasis)),
            }
            index = end;
        }
        tidy_inline(&out)
    }
}

fn render_emphasis(spans: &[Span], emphasis: bool) -> String {
    let mut out = String::new();
    // Merge neighbours with identical emphasis (links already grouped).
    let mut merged: Vec<(bool, bool, String)> = Vec::new();
    for span in spans {
        let (bold, italic) = if emphasis {
            (span.bold, span.italic)
        } else {
            (false, false)
        };
        // Whitespace-only spans inherit the previous formatting so markers do not split.
        let (bold, italic) = if span.text.trim().is_empty() {
            merged
                .last()
                .map(|(b, i, _)| (*b, *i))
                .unwrap_or((bold, italic))
        } else {
            (bold, italic)
        };
        match merged.last_mut() {
            Some((b, i, text)) if *b == bold && *i == italic => text.push_str(&span.text),
            _ => merged.push((bold, italic, span.text.clone())),
        }
    }
    for (bold, italic, text) in merged {
        let (lead, core, trail) = split_ws(&text);
        if core.is_empty() || (!bold && !italic) {
            out.push_str(&text);
            continue;
        }
        let (open, close) = match (bold, italic) {
            (true, true) => ("**_", "_**"),
            (true, false) => ("**", "**"),
            _ => ("_", "_"),
        };
        out.push_str(lead);
        // Markers cannot wrap line breaks; apply them per line.
        let lines: Vec<String> = core
            .split('\n')
            .map(|line| {
                let (l, c, t) = split_ws(line);
                if c.is_empty() {
                    line.to_string()
                } else {
                    format!("{l}{open}{c}{close}{t}")
                }
            })
            .collect();
        out.push_str(&lines.join("\n"));
        out.push_str(trail);
    }
    out
}

fn split_ws(s: &str) -> (&str, &str, &str) {
    let start = s.len() - s.trim_start().len();
    let end = s.trim_end().len().max(start);
    (&s[..start], &s[start..end], &s[end..])
}

/// Collapse runs of spaces, trim line ends, and drop empty lines inside a paragraph.
fn tidy_inline(s: &str) -> String {
    let mut lines = Vec::new();
    for line in s.split('\n') {
        let mut out = String::with_capacity(line.len());
        let mut prev_space = false;
        for c in line.chars() {
            let is_space = c == ' ' || c == '\u{a0}';
            if is_space {
                if !prev_space {
                    out.push(' ');
                }
            } else {
                out.push(c);
            }
            prev_space = is_space;
        }
        lines.push(out.trim().to_string());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    lines.join("\n")
}

/// Markdown for an image with alt text; images without alt text are dropped.
pub(crate) fn image_markdown(alt: &str, target: Option<&str>) -> Option<String> {
    let alt = collapse_ws(alt);
    if alt.is_empty() {
        return None;
    }
    let name = target
        .and_then(|t| t.rsplit('/').next())
        .filter(|n| !n.is_empty())
        .unwrap_or("image");
    Some(format!(
        "![{}]({})",
        alt.replace('[', "\\[").replace(']', "\\]"),
        name.replace(' ', "%20")
    ))
}

/// Join Markdown blocks: list items stay tight, everything else gets a blank line.
pub(crate) struct Blocks {
    out: String,
    last_was_list: bool,
}

impl Blocks {
    pub(crate) fn new() -> Self {
        Self {
            out: String::new(),
            last_was_list: false,
        }
    }

    pub(crate) fn push(&mut self, block: &str, is_list: bool) {
        let block = block.trim_end();
        if block.trim().is_empty() {
            return;
        }
        if !self.out.is_empty() {
            self.out.push_str(if is_list && self.last_was_list {
                "\n"
            } else {
                "\n\n"
            });
        }
        self.out.push_str(block);
        self.last_was_list = is_list;
    }

    pub(crate) fn finish(self) -> String {
        self.out
    }
}

/// Tracks list marker widths so nested items indent under their parent's content.
#[derive(Default)]
pub(crate) struct ListIndent {
    widths: Vec<usize>,
}

impl ListIndent {
    pub(crate) fn reset(&mut self) {
        self.widths.clear();
    }

    /// Format an item at `level` with `marker` (e.g. "-", "3.").
    pub(crate) fn item(&mut self, level: usize, marker: &str, text: &str) -> String {
        let level = level.min(8);
        self.widths.truncate(level);
        while self.widths.len() < level {
            self.widths.push(2);
        }
        let indent = " ".repeat(self.widths.iter().sum());
        self.widths.push(marker.chars().count() + 1);
        let continuation = format!("\n{}", " ".repeat(self.widths.iter().sum()));
        format!("{indent}{marker} {}", text.replace('\n', &continuation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(
            resolve_target("ppt/slides", "../notesSlides/n1.xml"),
            "ppt/notesSlides/n1.xml"
        );
        assert_eq!(resolve_target("word", "media/a.png"), "word/media/a.png");
        assert_eq!(resolve_target("word", "/word/x.xml"), "word/x.xml");
        assert_eq!(resolve_target("", "word/document.xml"), "word/document.xml");
    }

    #[test]
    fn inline_merges_and_places_markers() {
        let mut inline = Inline::default();
        inline.push("Hello ", true, false, None);
        inline.push("world", true, false, None);
        inline.push(" and ", false, false, None);
        inline.push("site", false, true, Some("https://x.y"));
        inline.push("   ", true, false, None);
        assert_eq!(
            inline.render(true),
            "**Hello world** and [_site_](https://x.y)"
        );
        assert_eq!(inline.render(false), "Hello world and [site](https://x.y)");
    }

    #[test]
    fn xml_entities_and_depth() {
        let root = parse_xml(b"<a x=\"1&amp;2\">x &lt; y &#x41;<![CDATA[<z>]]></a>").unwrap();
        assert_eq!(root.attr("x"), Some("1&2"));
        assert_eq!(root.text(), "x < y A<z>");
        let deep = "<a>".repeat(2000);
        assert!(parse_xml(deep.as_bytes()).is_err());
        assert!(parse_xml(b"").is_err());
    }

    #[test]
    fn list_indent_nests_under_content() {
        let mut list = ListIndent::default();
        assert_eq!(list.item(0, "10.", "a"), "10. a");
        assert_eq!(list.item(1, "-", "b"), "    - b");
        assert_eq!(list.item(0, "11.", "c\nd"), "11. c\n    d");
    }
}
