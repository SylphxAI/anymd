use super::{valid_cite_box, PositionedTextItem, TextBoundingBox, CITE_ROUNDING_TOLERANCE};

pub(super) struct Geometry<'a> {
    items: &'a [PositionedTextItem],
    item_ranges: Vec<(u32, u32)>,
    bounds: TextBoundingBox,
    coarse_level: &'a str,
    coarse_levels: Option<&'a [&'a str]>,
}

impl<'a> Geometry<'a> {
    pub(super) fn new(
        items: &'a [PositionedTextItem],
        bounds: TextBoundingBox,
        separator: &str,
        coarse_level: &'a str,
        coarse_levels: Option<&'a [&'a str]>,
    ) -> Self {
        let mut item_ranges = Vec::new();
        let mut offset = 0u32;
        for item in items {
            let end = offset + item.text.encode_utf16().count() as u32;
            item_ranges.push((offset, end));
            offset = end + separator.encode_utf16().count() as u32;
        }
        Self {
            items,
            item_ranges,
            bounds,
            coarse_level,
            coarse_levels,
        }
    }

    pub(super) fn support(
        &self,
        start: u32,
        end: u32,
    ) -> Option<(Vec<TextBoundingBox>, Vec<String>)> {
        let mut boxes = Vec::new();
        let mut levels = Vec::new();
        for (index, (item, &(item_start, item_end))) in
            self.items.iter().zip(&self.item_ranges).enumerate()
        {
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
                if !contains(self.bounds, b) {
                    return None;
                }
                boxes.push(b);
                levels.push(
                    self.coarse_levels
                        .and_then(|all| all.get(index))
                        .copied()
                        .unwrap_or(self.coarse_level)
                        .to_string(),
                );
            } else {
                let mut char_offset = 0u32;
                let mut item_box: Option<TextBoundingBox> = None;
                for ch in item.text.chars() {
                    let char_end = char_offset + ch.len_utf16() as u32;
                    if char_end > local_start && char_offset < local_end && !ch.is_whitespace() {
                        let b = covered_box(item, char_offset, char_end)?;
                        if !contains(self.bounds, b) {
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
    }

    pub(super) fn is_complete(&self) -> bool {
        // A negative needs complete text AND geometry. Coarse boxes intersecting
        // but not contained in the requested location cannot establish absence.
        self.items.iter().all(|item| {
            if item.chars.is_empty() {
                item.bounding_box.is_some_and(|b| contains(self.bounds, b))
            } else {
                let mut offset = 0;
                item.text.chars().all(|ch| {
                    let end = offset + ch.len_utf16() as u32;
                    let covered = ch.is_whitespace() || covered_box(item, offset, end).is_some();
                    offset = end;
                    covered
                })
            }
        })
    }
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
