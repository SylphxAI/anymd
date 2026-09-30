//! Deterministic navigation over the same Markdown units used by read and search.
use crate::document::{OpenOptions, Opened, Unit};
use crate::schema::OutlineArgs;
use crate::source_access::SourceAccessPolicy;
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;

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
    pub markdown: String,
    #[serde(skip)]
    pub units: Vec<UnitRange>,
}

impl Outline {
    pub fn build(opened: &Opened) -> Result<Self, String> {
        let units = opened.all_units()?;
        Ok(Self::from_units(opened, &units))
    }

    pub fn from_units(opened: &Opened, units: &[Unit]) -> Self {
        let markers = opened.total > 1 || opened.is_paged();
        let mut markdown = String::new();
        let mut ranges = Vec::new();
        for unit in units {
            let marker = if markers {
                format!("<!-- {} -->\n\n", unit.label)
            } else {
                String::new()
            };
            markdown.push_str(&marker);
            let start = markdown.len();
            let content = unit.markdown.trim();
            let offset = unit.markdown.len() - unit.markdown.trim_start().len();
            markdown.push_str(content);
            let end = markdown.len();
            markdown.push_str("\n\n");
            ranges.push(UnitRange {
                number: unit.number,
                start,
                end,
                offset,
            });
        }
        let mut nodes = vec![Node {
            id: "n1".into(),
            title: opened.title.clone().unwrap_or_else(|| "Document".into()),
            level: 0,
            from: 1,
            to: opened.total,
            start: 0,
            end: markdown.len(),
            children: 0,
            path: String::new(),
        }];
        let has_headings = units.iter().any(|u| !headings(&u.markdown).is_empty());
        let bookmarks: Vec<_> = opened
            .outline()
            .into_iter()
            .filter_map(|(depth, title, page)| {
                let range = ranges.iter().find(|r| Some(r.number) == page)?;
                let start = if opened.format == "epub" {
                    units
                        .iter()
                        .find(|u| u.number == range.number)
                        .and_then(|u| {
                            headings(&u.markdown)
                                .into_iter()
                                .find(|(_, heading, _)| heading == &title)
                        })
                        .map(|(_, _, offset)| range.start + offset.saturating_sub(range.offset))
                        .unwrap_or(range.start)
                } else {
                    range.start
                };
                Some((depth + 1, title, range.number, start))
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
                    to: opened.total,
                    start,
                    end: markdown.len(),
                    children: 0,
                    path: String::new(),
                });
            }
        } else {
            for (unit, range) in units.iter().zip(&ranges) {
                let headings = headings(&unit.markdown);
                let unit_node =
                    opened.unit_noun == "slide" || opened.unit_noun == "chapter" || !has_headings;
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
                        to: opened.total,
                        start: range.start + offset.saturating_sub(range.offset),
                        end: markdown.len(),
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
            if !native && (opened.unit_noun == "slide" || opened.unit_noun == "chapter") {
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
            source: opened.label.clone(),
            format: opened.format.into(),
            unit: opened.unit_noun.into(),
            nodes,
            markdown,
            units: ranges,
        }
    }

    pub fn node_at(&self, unit: u32, offset: usize) -> &Node {
        let position = self
            .units
            .iter()
            .find(|r| r.number == unit)
            .map(|r| r.start + offset.saturating_sub(r.offset))
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
fn headings(markdown: &str) -> Vec<(usize, String, usize)> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut previous: Option<(&str, usize)> = None;
    let mut offset = 0;
    for line in markdown.split_inclusive('\n') {
        let text = line.trim_end();
        let trimmed = text.trim_start();
        let indent = text.len() - trimmed.len();
        let first = trimmed.chars().next().unwrap_or(' ');
        let run = trimmed.chars().take_while(|&c| c == first).count();
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

pub fn render(args: &OutlineArgs, policy: &SourceAccessPolicy) -> Result<String, String> {
    if !matches!(args.format.as_deref(), None | Some("json" | "tree")) {
        return Err("format must be json or tree".into());
    }
    if !matches!(args.images.as_deref(), None | Some("none" | "refs")) {
        return Err("images must be none or refs".into());
    }
    let revisions = match args.revisions.as_deref() {
        Some(value) => anymd_formats::Revisions::parse(value)
            .ok_or("revisions must be markup, accept or reject")?,
        None => anymd_formats::Revisions::default(),
    };
    let options = OpenOptions {
        ocr: Some(args.ocr.unwrap_or(false)),
        images: if args.images.as_deref() == Some("refs") {
            anymd_formats::images::ImageStore::default_location()
        } else {
            None
        },
        revisions,
        ..Default::default()
    };
    let opened = Opened::open(&args.source, policy, &options)?;
    let outline = Outline::build(&opened)?;
    if args.format.as_deref() == Some("tree") {
        Ok(outline.tree())
    } else {
        serde_json::to_string_pretty(&outline).map_err(|e| e.to_string())
    }
}

pub fn tool(
    args: &OutlineArgs,
    policy: &SourceAccessPolicy,
) -> Result<CallToolResult, rmcp::ErrorData> {
    render(args, policy)
        .map(|text| CallToolResult::success(vec![ContentBlock::text(text)]))
        .map_err(|message| rmcp::ErrorData::invalid_params(message, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headings_ignore_code_and_keep_utf8_offsets() {
        let text = "# 中文\n\n```md\n# not a heading\n```\n\nRevenue\n-------\n## Costs ##\n";
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
    fn existing_format_fixtures_have_readable_stable_nodes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures");
        for file in [
            "sample.pdf",
            "differential/v3014-behavior-v1.pdf",
            "alt-text.docx",
            "slides.pptx",
            "field-notes.epub",
        ] {
            let opened = Opened::open(
                root.join(file).to_str().unwrap(),
                &SourceAccessPolicy::default(),
                &OpenOptions {
                    ocr: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
            let outline = Outline::build(&opened).unwrap();
            assert!(outline.nodes.len() > 1, "{file}");
            assert_eq!(outline.tree(), Outline::build(&opened).unwrap().tree());
            for node in &outline.nodes {
                assert!(
                    node.start <= node.end && node.end <= outline.markdown.len(),
                    "{file}: {}",
                    node.id
                );
                assert!(
                    outline.markdown.is_char_boundary(node.start)
                        && outline.markdown.is_char_boundary(node.end)
                );
                assert!(node.from <= node.to);
            }
        }
    }
}
