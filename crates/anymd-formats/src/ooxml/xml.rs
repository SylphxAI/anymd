use quick_xml::events::Event;
use quick_xml::Reader;

use super::package::Package;
use super::render::collapse_ws;

/// Deepest XML nesting we accept; deeper input is treated as hostile.
const MAX_DEPTH: usize = 512;

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
                let s = t.xml_content(quick_xml::XmlVersion::Implicit1_0);
                push_text(&mut stack, &s);
            }
            Event::CData(t) => {
                let s = t.xml_content(quick_xml::XmlVersion::Implicit1_0);
                push_text(&mut stack, &s);
            }
            Event::GeneralRef(r) => {
                let resolved = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => {
                        let name: &str = &r;
                        match name {
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
    let name = start.name().as_ref().to_string();
    let mut attrs = Vec::new();
    for attr in start.attributes().with_checks(false) {
        let attr = attr.map_err(|e| e.to_string())?;
        let key = attr.key.as_ref().to_string();
        let value = match attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
            Ok(v) => v.into_owned(),
            Err(_) => attr.value.to_string(),
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
