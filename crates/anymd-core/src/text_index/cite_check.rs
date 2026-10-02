//! Deterministic quote/location support. This is not a semantic truth check.
use super::{PositionedTextItem, TextBoundingBox};
use serde::{Deserialize, Serialize};

/// Coordinates are rounded to 1/10,000 PDF unit by the text owner. Allow only
/// that rounding error, never a search radius or configurable padding.
pub const CITE_ROUNDING_TOLERANCE: f64 = 0.0001;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CiteNormalization {
    #[default]
    None,
    WhitespaceV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiteSupport {
    pub observed_text: String,
    /// Original reading-order UTF-16 offsets, not normalized offsets.
    pub start: u32,
    pub end: u32,
    pub boxes: Vec<TextBoundingBox>,
    pub geometry_level: String,
    /// One granularity label per supporting box; mixed spans never lose labels.
    pub geometry_levels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiteDecision {
    pub verdict: String,
    pub locations: Vec<CiteSupport>,
}

pub fn valid_cite_box(b: TextBoundingBox) -> bool {
    [b.left, b.bottom, b.right, b.top]
        .into_iter()
        .all(f64::is_finite)
        && b.right > b.left
        && b.top > b.bottom
}

fn contains(outer: TextBoundingBox, inner: TextBoundingBox) -> bool {
    valid_cite_box(inner)
        && inner.left >= outer.left - CITE_ROUNDING_TOLERANCE
        && inner.bottom >= outer.bottom - CITE_ROUNDING_TOLERANCE
        && inner.right <= outer.right + CITE_ROUNDING_TOLERANCE
        && inner.top <= outer.top + CITE_ROUNDING_TOLERANCE
}

fn covered_box(item: &PositionedTextItem, start: u32, end: u32) -> Option<TextBoundingBox> {
    let index = item.chars.partition_point(|g| g.item_char_end <= start);
    let g = item.chars.get(index)?;
    (g.item_char_start <= start && g.item_char_end >= end)
        .then_some(g.bounding_box)
        .flatten()
        .filter(|b| valid_cite_box(*b))
}

// Each normalized UTF-8 byte maps to the complete original character/run.
fn whitespace_map(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut output = String::new();
    let mut map = Vec::new();
    let mut pending = None;
    for (start, ch) in text.char_indices() {
        let end = start + ch.len_utf8();
        if ch.is_whitespace() {
            pending = Some((pending.map_or(start, |(s, _)| s), end));
        } else {
            if !output.is_empty() {
                if let Some(span) = pending.take() {
                    output.push(' ');
                    map.push(span);
                }
            }
            pending = None;
            output.push(ch);
            map.extend(std::iter::repeat_n((start, end), ch.len_utf8()));
        }
    }
    (output, map)
}

/// Items are contiguous in the existing text-index reading order. A coarse
/// item's whole box must fit; native estimated characters must cover every
/// contributing non-whitespace UTF-16 unit. Partial unions never prove support.
pub fn check_cite_items(
    items: &[PositionedTextItem],
    quote: &str,
    bounds: TextBoundingBox,
    normalization: CiteNormalization,
    complete: bool,
    coarse_level: &str,
) -> CiteDecision {
    check_cite_items_with_separator(
        items,
        quote,
        bounds,
        normalization,
        complete,
        coarse_level,
        "\n",
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn check_cite_items_with_separator(
    items: &[PositionedTextItem],
    quote: &str,
    bounds: TextBoundingBox,
    normalization: CiteNormalization,
    complete: bool,
    coarse_level: &str,
    separator: &str,
    coarse_levels: Option<&[&str]>,
) -> CiteDecision {
    let text = items
        .iter()
        .map(|i| i.text.as_str())
        .collect::<Vec<_>>()
        .join(separator);
    let mut item_ranges = Vec::new();
    let mut offset = 0u32;
    for item in items {
        let end = offset + item.text.encode_utf16().count() as u32;
        item_ranges.push((offset, end));
        offset = end + separator.encode_utf16().count() as u32;
    }
    let geometry = |start: u32, end: u32| -> Option<(Vec<TextBoundingBox>, Vec<String>)> {
        let mut boxes = Vec::new();
        let mut levels = Vec::new();
        for (index, (item, &(item_start, item_end))) in items.iter().zip(&item_ranges).enumerate() {
            if end <= item_start || start >= item_end {
                continue;
            }
            let local_start = start.saturating_sub(item_start);
            let local_end = (end - item_start).min(item_end - item_start);
            if item.chars.is_empty() {
                let mut offset = 0;
                if !item.text.chars().any(|ch| {
                    let end = offset + ch.len_utf16() as u32;
                    let contributes =
                        end > local_start && offset < local_end && !ch.is_whitespace();
                    offset = end;
                    contributes
                }) {
                    continue;
                }
                let b = item.bounding_box?;
                if !contains(bounds, b) {
                    return None;
                }
                boxes.push(b);
                levels.push(
                    coarse_levels
                        .and_then(|all| all.get(index))
                        .copied()
                        .unwrap_or(coarse_level)
                        .to_string(),
                );
            } else {
                let mut char_offset = 0u32;
                let mut item_box: Option<TextBoundingBox> = None;
                for ch in item.text.chars() {
                    let char_end = char_offset + ch.len_utf16() as u32;
                    if char_end > local_start && char_offset < local_end && !ch.is_whitespace() {
                        let b = covered_box(item, char_offset, char_end)?;
                        if !contains(bounds, b) {
                            return None;
                        }
                        item_box = Some(match item_box {
                            Some(current) => current.union(b)?,
                            None => b,
                        });
                    }
                    char_offset = char_end;
                }
                if let Some(b) = item_box {
                    boxes.push(b);
                    levels.push("char_estimated".into());
                }
            }
        }
        (!boxes.is_empty()).then_some((boxes, levels))
    };
    for normalized in [false, true] {
        if normalized && normalization != CiteNormalization::WhitespaceV1 {
            break;
        }
        let (haystack, map) = if normalized {
            whitespace_map(&text)
        } else {
            (text.clone(), Vec::new())
        };
        let needle = if normalized {
            whitespace_map(quote).0
        } else {
            quote.into()
        };
        if needle.is_empty() {
            continue;
        }
        let mut locations = Vec::new();
        let mut from = 0;
        while let Some(relative) = haystack[from..].find(&needle) {
            let s = from + relative;
            let e = s + needle.len();
            let (original_start, original_end) = if normalized {
                (map[s].0, map[e - 1].1)
            } else {
                (s, e)
            };
            let start = text[..original_start].encode_utf16().count() as u32;
            let end = text[..original_end].encode_utf16().count() as u32;
            if let Some((boxes, geometry_levels)) = geometry(start, end) {
                let geometry_level = if geometry_levels
                    .iter()
                    .all(|level| level == &geometry_levels[0])
                {
                    geometry_levels[0].clone()
                } else {
                    "mixed".into()
                };
                locations.push(CiteSupport {
                    observed_text: text[original_start..original_end].into(),
                    start,
                    end,
                    boxes,
                    geometry_level,
                    geometry_levels,
                });
                // Presence, not uniqueness: output bounded locations.
                if locations.len() == 8 {
                    break;
                }
            }
            from = s + haystack[s..].chars().next().unwrap().len_utf8();
        }
        if !locations.is_empty() {
            return CiteDecision {
                verdict: if normalized {
                    "verified_normalized"
                } else {
                    "verified_exact"
                }
                .into(),
                locations,
            };
        }
    }
    // A negative needs complete text AND geometry. Coarse boxes intersecting
    // but not contained in the requested location cannot establish absence.
    let geometry_complete = items.iter().all(|item| {
        if item.chars.is_empty() {
            item.bounding_box.is_some_and(|b| contains(bounds, b))
        } else {
            let mut offset = 0;
            item.text.chars().all(|ch| {
                let end = offset + ch.len_utf16() as u32;
                let covered = ch.is_whitespace() || covered_box(item, offset, end).is_some();
                offset = end;
                covered
            })
        }
    });
    CiteDecision {
        verdict: if complete && !text.trim().is_empty() && geometry_complete {
            "unmatched"
        } else {
            "insufficient_evidence"
        }
        .into(),
        locations: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_index::TextCharacterGeometry;
    fn bounds() -> TextBoundingBox {
        TextBoundingBox {
            left: 0.,
            bottom: 0.,
            right: 100.,
            top: 100.,
        }
    }
    fn item(text: &str) -> PositionedTextItem {
        let mut offset = 0;
        let chars = text
            .chars()
            .map(|ch| {
                let start = offset;
                offset += ch.len_utf16() as u32;
                TextCharacterGeometry {
                    text: ch.to_string(),
                    item_char_start: start,
                    item_char_end: offset,
                    is_whitespace: ch.is_whitespace(),
                    bounding_box: Some(bounds()),
                }
            })
            .collect();
        PositionedTextItem {
            text: text.into(),
            bounding_box: Some(bounds()),
            chars,
            runs: vec![],
        }
    }
    #[test]
    fn cite_check_repeated_quotes_ligatures_and_rounding_bounds() {
        let items = vec![item("word word ﬁle")];
        let result = check_cite_items(
            &items,
            "word",
            bounds(),
            CiteNormalization::None,
            true,
            "text_item",
        );
        assert_eq!(result.locations.len(), 2);
        assert_eq!(
            check_cite_items(
                &items,
                "file",
                bounds(),
                CiteNormalization::WhitespaceV1,
                true,
                "text_item"
            )
            .verdict,
            "unmatched"
        );
        let too_small = TextBoundingBox {
            right: 99.999,
            ..bounds()
        };
        assert_eq!(
            check_cite_items(
                &items,
                "word",
                too_small,
                CiteNormalization::None,
                true,
                "text_item"
            )
            .verdict,
            "unmatched"
        );
        assert!(!valid_cite_box(TextBoundingBox {
            right: f64::INFINITY,
            ..bounds()
        }));
        assert!(!valid_cite_box(TextBoundingBox {
            top: 0.,
            ..bounds()
        }));
    }

    #[test]
    fn cite_check_preserves_mixed_geometry_labels() {
        let mut coarse = item("second");
        coarse.chars.clear();
        let result = check_cite_items(
            &[item("first"), coarse],
            "first\nsecond",
            bounds(),
            CiteNormalization::None,
            false,
            "text_item",
        );
        assert_eq!(result.verdict, "verified_exact");
        assert_eq!(result.locations[0].geometry_level, "mixed");
        assert_eq!(
            result.locations[0].geometry_levels,
            ["char_estimated", "text_item"]
        );
    }

    #[test]
    fn cite_check_exact_negative_and_incomplete() {
        let items = vec![item("A😀B")];
        let run = |q| {
            check_cite_items(
                &items,
                q,
                bounds(),
                CiteNormalization::None,
                true,
                "text_item",
            )
        };
        let result = run("😀B");
        assert_eq!(result.verdict, "verified_exact");
        assert_eq!(result.locations[0].start, 1);
        assert_eq!(result.locations[0].end, 4);
        for q in ["a😀B", "A B", "A😀B!", "AB"] {
            assert_eq!(run(q).verdict, "unmatched");
        }
        assert_eq!(
            check_cite_items(
                &[],
                "quote",
                bounds(),
                CiteNormalization::None,
                true,
                "text_item"
            )
            .verdict,
            "insufficient_evidence"
        );
        let mut partial = items.clone();
        partial[0].chars[1].bounding_box = None;
        assert_eq!(
            check_cite_items(
                &partial,
                "A😀B",
                bounds(),
                CiteNormalization::None,
                true,
                "text_item"
            )
            .verdict,
            "insufficient_evidence"
        );
        assert_eq!(
            check_cite_items(
                &partial,
                "A",
                bounds(),
                CiteNormalization::None,
                false,
                "text_item"
            )
            .verdict,
            "verified_exact"
        );
    }
    #[test]
    fn cite_check_whitespace_maps_original_spans_across_lines() {
        let items = vec![item("😀  first"), item("second\tword")];
        let result = check_cite_items(
            &items,
            "first second word",
            bounds(),
            CiteNormalization::WhitespaceV1,
            true,
            "text_item",
        );
        assert_eq!(result.verdict, "verified_normalized");
        assert_eq!(result.locations[0].observed_text, "first\nsecond\tword");
        assert_eq!(result.locations[0].start, 4);
        assert_eq!(
            check_cite_items(
                &items,
                "first second word",
                bounds(),
                CiteNormalization::None,
                true,
                "text_item"
            )
            .verdict,
            "unmatched"
        );
    }
    #[test]
    fn cite_check_coarse_regions_and_wrong_location() {
        let mut region = item("word");
        region.chars.clear();
        let small = TextBoundingBox {
            right: 50.,
            ..bounds()
        };
        assert_eq!(
            check_cite_items(
                &[region.clone()],
                "word",
                small,
                CiteNormalization::None,
                true,
                "ocr_region"
            )
            .verdict,
            "insufficient_evidence"
        );
        let result = check_cite_items(
            &[region],
            "word",
            bounds(),
            CiteNormalization::None,
            false,
            "ocr_region",
        );
        assert_eq!(result.verdict, "verified_exact");
        assert_eq!(result.locations[0].geometry_level, "ocr_region");
        assert_eq!(
            check_cite_items(
                &[item("word")],
                "word",
                small,
                CiteNormalization::None,
                true,
                "text_item"
            )
            .verdict,
            "unmatched"
        );
    }
}
