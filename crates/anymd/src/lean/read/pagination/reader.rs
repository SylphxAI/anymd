//! Convert selected units in chunks and stop at a token-budget boundary.

use anymd_core::{document::PAGE_CHUNK, outline::unit_marker};

use crate::document::Opened;
use crate::lean::{estimate_tokens, scan_marker};

use super::response::{failed, finish, plural};
use super::selection::{clip_bounds, wanted_pages, Navigation};
use super::{cut_point, Cursor, SourceRead};

/// Read one opened document into Markdown within `budget` tokens.
pub(in crate::lean) fn read_opened(
    opened: &mut Opened,
    selection: Option<Vec<u32>>,
    cursor: Option<Cursor>,
    budget: usize,
    first_call: bool,
    node_id: Option<&str>,
) -> SourceRead {
    let navigation = match node_id.map(|id| Navigation::find(opened, id)).transpose() {
        Ok(navigation) => navigation,
        Err(message) => return failed(&opened.label, message),
    };
    let wanted = wanted_pages(opened.total, selection, cursor, navigation.as_ref());
    if wanted.is_empty() {
        let total = opened.total;
        let message = format!(
            "No requested {} exist (the document has {total}).",
            plural(opened.unit_noun, 2)
        );
        return failed(&opened.label, message);
    }
    // One-unit documents (a web page, a DOCX, an image) need no unit markers.
    let markers = opened.info().has_markers();

    let mut body = String::new();
    let mut shown: Vec<u32> = Vec::new();
    let mut next = None;
    let mut used = 0usize;
    let mut visible = 0usize;
    let mut scanned: Vec<u32> = Vec::new();
    let chunk_size = if opened.is_paged() {
        PAGE_CHUNK
    } else {
        usize::MAX
    };
    'chunks: for chunk in wanted.chunks(chunk_size.min(wanted.len()).max(1)) {
        let units = match opened.units(chunk) {
            Ok(units) => units,
            Err(message) => return failed(&opened.label, message),
        };
        opened.title_from_units(&units);
        let chunk_scans = opened.scanned_pages(&units);
        for unit in units {
            let Some((clip_start, clip_end)) = clip_bounds(navigation.as_ref(), &unit) else {
                continue;
            };
            let mut skip = match cursor {
                Some(cursor) if cursor.page == unit.number => {
                    cursor.offset.min(unit.markdown.len())
                }
                _ => 0,
            };
            skip = skip.max(clip_start).min(clip_end);
            while !unit.markdown.is_char_boundary(skip) {
                skip -= 1;
            }
            let content = unit.markdown[skip..clip_end].trim_start();
            let visible_text = if navigation.is_some() {
                unit.markdown.as_str()
            } else {
                content
            };
            visible += visible_text.chars().filter(|c| c.is_alphanumeric()).count();
            let continued = if navigation.is_some() {
                cursor.is_some_and(|cursor| cursor.page == unit.number && cursor.offset > 0)
            } else {
                skip > 0
            };
            let mut marker = match (markers, continued) {
                (false, false) => String::new(),
                (false, true) => "<!-- continued -->\n\n".to_string(),
                (true, false) => unit_marker(&unit.label),
                (true, true) => unit_marker(&format!("{} (continued)", unit.label)),
            };
            let is_scan = chunk_scans.contains(&unit.number) && !continued;
            if is_scan {
                marker.push_str(&scan_marker(unit.number, opened.ocr_ran(unit.number)));
            }
            // A scan page is charged its page label and scan marker, plus its
            // entry in the `scanned_pages` header; text pages keep the old cost.
            let piece_tokens = if is_scan {
                estimate_tokens(content) + estimate_tokens(&marker) + 4
            } else {
                estimate_tokens(content) + 8
            };
            if used + piece_tokens > budget {
                if shown.is_empty() {
                    // A unit larger than the whole budget: cut inside it.
                    let room_tokens = budget.saturating_sub(used + 8).max(250);
                    let room = content.len() * room_tokens / piece_tokens.max(1);
                    let cut = cut_point(content, room.max(512));
                    body.push_str(&marker);
                    body.push_str(content[..cut].trim_end());
                    body.push_str("\n\n");
                    shown.push(unit.number);
                    if is_scan {
                        scanned.push(unit.number);
                    }
                    let consumed = clip_end - content.len() + cut;
                    if cut < content.len() {
                        next = Some(Cursor {
                            page: unit.number,
                            offset: consumed,
                        });
                    } else if let Some(following) = wanted.iter().find(|p| **p > unit.number) {
                        next = Some(Cursor {
                            page: *following,
                            offset: 0,
                        });
                    }
                } else {
                    next = Some(Cursor {
                        page: unit.number,
                        offset: skip,
                    });
                }
                break 'chunks;
            }
            body.push_str(&marker);
            body.push_str(content.trim_end());
            body.push_str("\n\n");
            shown.push(unit.number);
            if is_scan {
                scanned.push(unit.number);
            }
            used += piece_tokens;
        }
    }

    finish(
        opened,
        body,
        &shown,
        &scanned,
        visible,
        next,
        first_call && navigation.is_none(),
    )
}
