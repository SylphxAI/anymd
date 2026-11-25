//! Navigation over the canonical Markdown body and its citable units.

use serde::Serialize;

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

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: String,
    pub title: String,
    pub level: usize,
    /// Inclusive 1-based page, slide, sheet or chapter range.
    pub from: u32,
    pub to: u32,
    /// Half-open UTF-8 byte range in the canonical Markdown body.
    pub start: usize,
    pub end: usize,
    /// Number of immediate children (nodes are in preorder).
    pub children: usize,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct UnitRange {
    pub number: u32,
    pub start: usize,
    pub end: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outline {
    pub source: String,
    pub format: String,
    pub unit: String,
    pub nodes: Vec<Node>,
    #[serde(skip)]
    pub units: Vec<UnitRange>,
}

impl Outline {
    /// Index the canonical body (excluding the front matter and any notices).
    /// Supply units in reading order, and native outline entries in preorder.
    pub fn from_units(
        info: DocumentInfo<'_>,
        units: &[Unit],
        entries: &[(usize, String, Option<u32>)],
    ) -> Self {
        let markers = info.has_markers();
        let mut length = 0usize;
        let mut ranges = Vec::new();
        for unit in units {
            let marker = if markers {
                format!("<!-- {} -->\n\n", unit.label)
            } else {
                String::new()
            };
            length += marker.len();
            let start = length;
            let content = unit.markdown.trim();
            let offset = unit.markdown.len() - unit.markdown.trim_start().len();
            length += content.len();
            let end = length;
            length += 2;
            ranges.push(UnitRange {
                number: unit.number,
                start,
                end,
                offset,
            });
        }
        let mut nodes = vec![Node {
            id: "n1".into(),
            title: info.title.unwrap_or("Document").to_string(),
            level: 0,
            from: 1,
            to: info.total,
            start: 0,
            end: length,
            children: 0,
            path: String::new(),
        }];
        let has_headings = units.iter().any(|u| !headings(&u.markdown).is_empty());
        let bookmarks: Vec<_> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, (depth, title, page))| {
                // Organizational bookmarks need not have a destination of their
                // own. Their first child supplies the start of the subtree.
                let page = page.or_else(|| {
                    entries[index + 1..]
                        .iter()
                        .take_while(|entry| entry.0 > *depth)
                        .find_map(|entry| entry.2)
                });
                let range = ranges.iter().find(|r| Some(r.number) == page)?;
                let start = if info.format == "epub" {
                    units
                        .iter()
                        .find(|u| u.number == range.number)
                        .and_then(|u| {
                            headings(&u.markdown)
                                .into_iter()
                                .find(|(_, heading, _)| heading == title)
                        })
                        .map(|(_, _, offset)| range.start + offset.saturating_sub(range.offset))
                        .unwrap_or(range.start)
                } else {
                    range.start
                };
                Some((depth + 1, title.clone(), range.number, start))
            })
            .collect();
        let native = !bookmarks.is_empty();
        if native {
            for (level, title, from, start) in bookmarks {
                nodes.push(Node {
                    id: String::new(),
                    title,
                    level,
                    from,
                    to: info.total,
                    start,
                    end: length,
                    children: 0,
                    path: String::new(),
                });
            }
        } else {
            for (unit, range) in units.iter().zip(&ranges) {
                let headings = headings(&unit.markdown);
                let unit_node =
                    info.unit_noun == "slide" || info.unit_noun == "chapter" || !has_headings;
                if unit_node {
                    let title = headings
                        .first()
                        .map(|(_, title, _)| title.clone())
                        .unwrap_or_else(|| unit.label.clone());
                    nodes.push(Node {
                        id: String::new(),
                        title,
                        level: 1,
                        from: unit.number,
                        to: unit.number,
                        start: range.start,
                        end: range.end,
                        children: 0,
                        path: String::new(),
                    });
                }
                for (level, title, offset) in headings {
                    // The first heading names a slide/chapter rather than duplicating it.
                    if unit_node && offset == range.offset {
                        continue;
                    }
                    nodes.push(Node {
                        id: String::new(),
                        title,
                        level: level + usize::from(unit_node),
                        from: unit.number,
                        to: info.total,
                        start: range.start + offset.saturating_sub(range.offset),
                        end: length,
                        children: 0,
                        path: String::new(),
                    });
                }
            }
        }
        // Close sections at the next sibling/ancestor and derive preorder ids.
        for i in 1..nodes.len() {
            let boundary = (i + 1..nodes.len()).find(|&j| nodes[j].level <= nodes[i].level);
            if let Some(j) = boundary {
                // Native TOCs can name several sections on one page. At page
                // granularity those ranges overlap rather than becoming empty.
                if nodes[j].start > nodes[i].start {
                    nodes[i].end = nodes[i].end.min(nodes[j].start);
                } else if let Some(range) = ranges.iter().find(|r| r.number == nodes[i].from) {
                    nodes[i].end = nodes[i].end.min(range.end);
                }
            }
            // Slide/chapter descendants cannot spill into the next unit.
            if !native && (info.unit_noun == "slide" || info.unit_noun == "chapter") {
                if let Some(range) = ranges.iter().find(|r| r.number == nodes[i].from) {
                    nodes[i].end = nodes[i].end.min(range.end);
                }
            }
            nodes[i].to = ranges
                .iter()
                .rev()
                .find(|r| r.start < nodes[i].end)
                .map(|r| r.number)
                .unwrap_or(nodes[i].from)
                .max(nodes[i].from);
            let parent = (0..i)
                .rev()
                .find(|&j| nodes[j].level < nodes[i].level)
                .unwrap_or(0);
            nodes[parent].children += 1;
            nodes[i].id = format!("{}.{}", nodes[parent].id, nodes[parent].children);
            nodes[i].path = if parent == 0 {
                nodes[i].title.clone()
            } else {
                format!("{} > {}", nodes[parent].path, nodes[i].title)
            };
        }
        nodes[0].path = nodes[0].title.clone();
        Self {
            source: info.source.into(),
            format: info.format.into(),
            unit: info.unit_noun.into(),
            nodes,
            units: ranges,
        }
    }

    pub fn node_at(&self, unit: u32, offset: usize) -> &Node {
        let position = self
            .units
            .iter()
            .find(|r| r.number == unit)
            .map(|r| r.start.saturating_add(offset.saturating_sub(r.offset)))
            .unwrap_or(0);
        self.nodes
            .iter()
            .rev()
            .find(|n| n.start <= position && position < n.end)
            .unwrap_or(&self.nodes[0])
    }

    pub fn tree(&self) -> String {
        let mut out = format!("{} ({}, {})\n", self.source, self.format, self.unit);
        for node in &self.nodes {
            out.push_str(&format!(
                "{}{} {} [{} {}-{}; bytes {}-{}; {} children]\n",
                "  ".repeat(node.level.min(32)),
                node.id,
                node.title.replace(['\n', '\r'], " "),
                self.unit,
                node.from,
                node.to,
                node.start,
                node.end,
                node.children
            ));
        }
        out
    }
}

/// ATX and Setext headings, excluding fenced code and indented code.
pub fn headings(markdown: &str) -> Vec<(usize, String, usize)> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut previous: Option<(&str, usize)> = None;
    let mut offset = 0;
    let mut comment = false;
    for line in markdown.split_inclusive('\n') {
        let text = line.trim_end();
        let trimmed = text.trim_start();
        let indent = text.len() - trimmed.len();
        let first = trimmed.chars().next().unwrap_or(' ');
        let run = trimmed.chars().take_while(|&c| c == first).count();
        if fence.is_none() && (comment || trimmed.starts_with("<!--")) {
            comment = !trimmed.contains("-->");
            previous = None;
            offset += line.len();
            continue;
        }
        if indent <= 3 && matches!(first, '`' | '~') && run >= 3 {
            if let Some((ch, length)) = fence {
                if first == ch && run >= length && trimmed[run..].trim().is_empty() {
                    fence = None;
                }
            } else {
                fence = Some((first, run));
            }
            previous = None;
        } else if fence.is_none() && indent <= 3 && !text.starts_with('\t') {
            if first == '#'
                && run <= 6
                && (trimmed.len() == run || trimmed.as_bytes()[run].is_ascii_whitespace())
            {
                let title = trimmed[run..].trim();
                let suffix = title.trim_end_matches('#');
                let title = if suffix.ends_with(char::is_whitespace) {
                    suffix.trim_end()
                } else {
                    title
                };
                if !title.is_empty() {
                    out.push((run, title.into(), offset));
                }
                previous = None;
            } else if matches!(first, '=' | '-') && run == trimmed.len() && run > 0 {
                if let Some((title, start)) = previous.take() {
                    out.push((if first == '=' { 1 } else { 2 }, title.into(), start));
                }
            } else {
                previous = (!trimmed.is_empty()).then_some((trimmed, offset));
            }
        } else {
            previous = None;
        }
        offset += line.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_ignore_code_and_keep_utf8_offsets() {
        let text = "# 中文\n\n```md\n# not a heading\n```\n\n<!--\n# hidden\n-->\n\nRevenue\n-------\n## Costs ##\n";
        let found = headings(text);
        assert_eq!(
            found.iter().map(|h| h.1.as_str()).collect::<Vec<_>>(),
            ["中文", "Revenue", "Costs"]
        );
        for (_, title, start) in found {
            assert!(text[start..].contains(&title));
        }
    }

    #[test]
    fn outline_ranges_align_with_the_utf8_canonical_body() {
        let units = [
            Unit {
                number: 1,
                label: "chapter 一".into(),
                markdown: "\u{3000}# 中文\n\nRésumé 😀\n".into(),
            },
            Unit {
                number: 2,
                label: "chapter 二".into(),
                markdown: "# 二\n\n## 日本語\n\n本文".into(),
            },
        ];
        let info = DocumentInfo {
            source: "book.epub",
            format: "epub",
            title: None,
            unit_noun: noun_for_units(&units, false),
            total: 2,
            paged: false,
        };
        assert_eq!(info.unit_noun, "chapter");
        let outline = Outline::from_units(info, &units, &[]);
        let again = Outline::from_units(info, &units, &[]);
        assert_eq!(outline.tree(), again.tree());
        assert_eq!(
            serde_json::to_value(&outline).unwrap(),
            serde_json::to_value(&again).unwrap()
        );
        let canonical = "<!-- chapter 一 -->\n\n# 中文\n\nRésumé 😀\n\n<!-- chapter 二 -->\n\n# 二\n\n## 日本語\n\n本文\n\n";
        assert_eq!(outline.nodes[0].end, canonical.len());
        for (range, unit) in outline.units.iter().zip(&units) {
            assert_eq!(&canonical[range.start..range.end], unit.markdown.trim());
        }
        for node in &outline.nodes {
            assert!(node.start <= node.end && node.end <= canonical.len());
            assert!(canonical.is_char_boundary(node.start) && canonical.is_char_boundary(node.end));
        }
        assert_eq!(outline.node_at(1, 3).title, "中文");
        let subheading = outline.node_at(2, "# 二\n\n".len());
        assert_eq!(subheading.path, "二 > 日本語");
        assert_eq!(
            &canonical[subheading.start..subheading.end],
            "## 日本語\n\n本文"
        );
    }
}
