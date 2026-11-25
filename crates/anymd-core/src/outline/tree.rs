use serde::Serialize;

use super::{sections::append_sections, unit_marker, DocumentInfo, Unit};

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
        let (ranges, length) = unit_ranges(info, units);
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
        let native = append_sections(&mut nodes, info, units, entries, &ranges, length);
        finalize_tree(&mut nodes, &ranges, info, native);
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

fn unit_ranges(info: DocumentInfo<'_>, units: &[Unit]) -> (Vec<UnitRange>, usize) {
    let markers = info.has_markers();
    let mut length = 0usize;
    let mut ranges = Vec::new();
    for unit in units {
        let marker = if markers {
            unit_marker(&unit.label)
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
    (ranges, length)
}

fn finalize_tree(nodes: &mut [Node], ranges: &[UnitRange], info: DocumentInfo<'_>, native: bool) {
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
}
