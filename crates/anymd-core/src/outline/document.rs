/// One citable unit of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// 1-based position used by `pages` selections and cursors.
    pub number: u32,
    /// Marker label: "page 3", "slide 2", "sheet Revenue", "chapter 4: …".
    pub label: String,
    pub markdown: String,
}

/// Facts needed to index and render a document, independent of its source adapter.
#[derive(Debug, Clone, Copy)]
pub struct DocumentInfo<'a> {
    pub source: &'a str,
    pub format: &'a str,
    pub title: Option<&'a str>,
    pub unit_noun: &'a str,
    pub total: u32,
    pub paged: bool,
}

impl DocumentInfo<'_> {
    /// Single-unit documents need no marker unless the unit is a PDF page.
    pub fn has_markers(self) -> bool {
        self.total > 1 || self.paged
    }

    /// The common header fields, before unit counts and reader-specific notices.
    pub fn header(self, metadata: &[(String, String)]) -> Vec<(String, String)> {
        let mut header = vec![("source".into(), self.source.into())];
        if self.format != "pdf" {
            header.push(("format".into(), self.format.into()));
        }
        if let Some(title) = self.title {
            header.push(("title".into(), title.into()));
        }
        for (key, value) in metadata.iter().take(6) {
            if key != "title" && key != "format" && !value.is_empty() && value.len() <= 200 {
                header.push((key.replace(' ', "_"), value.clone()));
            }
        }
        header
    }
}

/// Format Markdown front matter. Values are quoted unless their key opts into
/// preformatted YAML; only opt in for values assembled from trusted data.
pub fn front_matter(header: &[(String, String)], raw_keys: &[&str]) -> String {
    let mut out = String::from("---\n");
    for (key, value) in header {
        out.push_str(key);
        out.push_str(": ");
        if raw_keys.contains(&key.as_str()) {
            out.push_str(value);
        } else {
            out.push_str(&crate::text_rules::yaml_value(value));
        }
        out.push('\n');
    }
    out.push_str("---\n\n");
    out
}

pub fn unit_marker(label: &str) -> String {
    format!("<!-- {label} -->\n\n")
}

/// Use a specific noun when the units are uniformly labeled.
pub fn noun_for_units(units: &[Unit], paged: bool) -> &'static str {
    let shared = units
        .first()
        .and_then(|unit| unit.label.split_whitespace().next());
    match shared.filter(|word| {
        units
            .iter()
            .all(|unit| unit.label.split_whitespace().next() == Some(*word))
    }) {
        Some("slide") => "slide",
        Some("sheet") => "sheet",
        Some("chapter") => "chapter",
        _ if paged => "page",
        _ => "section",
    }
}

/// Fallback title from the first level-one ATX heading of unit 1.
pub fn title_from_units(units: &[Unit]) -> Option<String> {
    units
        .iter()
        .find(|unit| unit.number == 1)?
        .markdown
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|title| title.trim().to_string())
}
