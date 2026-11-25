//! Expand XHTML self-closing tags before parsing as HTML.

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// XHTML allows `<a id="x"/>`; an HTML parser reads that as an open `<a>` that
/// swallows the rest of the page. Rewrite non-void self-closing tags as pairs.
pub(in crate::html) fn expand_self_closing(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("/>") {
        return std::borrow::Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 64);
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' || !bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        let name_end = (i + 1..bytes.len())
            .find(|&j| !(bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-' || bytes[j] == b':'))
            .unwrap_or(bytes.len());
        // Find the tag end, honouring quoted attribute values.
        let mut quote = 0u8;
        let mut end = None;
        for (j, &b) in bytes.iter().enumerate().skip(name_end) {
            match (quote, b) {
                (0, b'"' | b'\'') => quote = b,
                (0, b'>') => {
                    end = Some(j);
                    break;
                }
                (q, b) if q == b => quote = 0,
                _ => {}
            }
        }
        let Some(end) = end else { break };
        let name = &text[i + 1..name_end];
        let local = name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase();
        if bytes[end - 1] == b'/' && !VOID_TAGS.contains(&local.as_str()) {
            out.push_str(&text[copied..end - 1]);
            out.push_str("></");
            out.push_str(name);
            out.push('>');
            copied = end + 1;
        }
        i = end + 1;
    }
    out.push_str(&text[copied..]);
    std::borrow::Cow::Owned(out)
}
