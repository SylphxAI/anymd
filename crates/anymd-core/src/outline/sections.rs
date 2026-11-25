//! Select section nodes from native bookmarks or Markdown headings.

use super::{headings, DocumentInfo, Node, Unit, UnitRange};

/// Append sections in preorder and report whether native bookmarks were used.
pub(super) fn append_sections(
    nodes: &mut Vec<Node>,
    info: DocumentInfo<'_>,
    units: &[Unit],
    entries: &[(usize, String, Option<u32>)],
    ranges: &[UnitRange],
    length: usize,
) -> bool {
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
        for (unit, range) in units.iter().zip(ranges) {
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
    native
}
