//! Select units by page, cursor, and outline node; map node spans into each unit.

use anymd_core::outline::Outline;

use crate::document::{Opened, Unit};

use super::Cursor;

pub(super) struct Navigation {
    outline: Outline,
    index: usize,
}

impl Navigation {
    pub(super) fn find(opened: &Opened, id: &str) -> Result<Self, String> {
        let outline = opened.build_outline()?;
        let index = outline
            .nodes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| format!("Unknown node {id}; call outline for this source."))?;
        Ok(Self { outline, index })
    }
}

pub(super) fn wanted_pages(
    total: u32,
    selection: Option<Vec<u32>>,
    cursor: Option<Cursor>,
    navigation: Option<&Navigation>,
) -> Vec<u32> {
    let mut wanted: Vec<u32> = match selection {
        Some(pages) => pages
            .into_iter()
            .filter(|page| *page >= 1 && *page <= total)
            .collect(),
        None => (1..=total).collect(),
    };
    if let Some(Navigation { outline, index }) = navigation {
        let node = &outline.nodes[*index];
        wanted.retain(|page| *page >= node.from && *page <= node.to);
    }
    if let Some(cursor) = cursor {
        wanted.retain(|page| *page >= cursor.page);
    }
    wanted
}

pub(super) fn clip_bounds(navigation: Option<&Navigation>, unit: &Unit) -> Option<(usize, usize)> {
    let Some(Navigation { outline, index }) = navigation else {
        return Some((0, unit.markdown.len()));
    };
    let node = &outline.nodes[*index];
    let range = outline.units.iter().find(|r| r.number == unit.number)?;
    let start = node.start.max(range.start);
    let end = node.end.min(range.end);
    if start >= end {
        return None;
    }
    Some((
        range.offset + start - range.start,
        range.offset + end - range.start,
    ))
}

pub(in crate::lean) fn describe_pages(pages: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < pages.len() {
        let start = pages[index];
        let mut end = start;
        while index + 1 < pages.len() && pages[index + 1] == end + 1 {
            index += 1;
            end = pages[index];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        index += 1;
    }
    parts.join(",")
}
