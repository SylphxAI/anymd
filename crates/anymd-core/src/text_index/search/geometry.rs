use crate::text_index::{PositionedTextItem, TextBoundingBox};

pub(in crate::text_index) struct TextGeometryIndex<'a> {
    pub(in crate::text_index) item: &'a PositionedTextItem,
    pub(in crate::text_index) eligible: Vec<usize>,
    #[cfg(test)]
    pub(in crate::text_index) candidate_visits: std::cell::Cell<usize>,
}

impl<'a> TextGeometryIndex<'a> {
    pub(in crate::text_index) fn new(item: &'a PositionedTextItem) -> Self {
        // Extraction emits monotonic half-open UTF-16 ranges. Exclude reversed
        // internal geometry fail-closed instead of making every match pay for
        // an unbounded two-dimensional malformed-range scan.
        let mut eligible = item
            .chars
            .iter()
            .enumerate()
            .filter(|(_, character)| {
                !character.is_whitespace
                    && character.item_char_end >= character.item_char_start
                    && character.bounding_box.is_some()
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if eligible
            .windows(2)
            .any(|pair| item.chars[pair[0]].item_char_start > item.chars[pair[1]].item_char_start)
        {
            eligible.sort_unstable_by_key(|index| {
                let character = &item.chars[*index];
                (character.item_char_start, character.item_char_end, *index)
            });
        }
        Self {
            item,
            eligible,
            #[cfg(test)]
            candidate_visits: std::cell::Cell::new(0),
        }
    }

    pub(in crate::text_index) fn range_candidates(
        &self,
        start_utf16: u32,
        end_utf16: u32,
    ) -> &[usize] {
        let first = self
            .eligible
            .partition_point(|index| self.item.chars[*index].item_char_start < start_utf16);
        let count = self.eligible[first..]
            .partition_point(|index| self.item.chars[*index].item_char_start <= end_utf16);
        &self.eligible[first..first + count]
    }

    pub(in crate::text_index) fn match_bounding_box(
        &self,
        start_utf16: u32,
        end_utf16: u32,
    ) -> (Option<TextBoundingBox>, Option<String>) {
        let char_box = self
            .range_candidates(start_utf16, end_utf16)
            .iter()
            .map(|index| {
                #[cfg(test)]
                self.candidate_visits
                    .set(self.candidate_visits.get().saturating_add(1));
                &self.item.chars[*index]
            })
            .filter(|character| character.item_char_end <= end_utf16)
            .filter_map(|character| character.bounding_box)
            .try_fold(None::<TextBoundingBox>, |current, box_| match current {
                None => Some(Some(box_)),
                Some(current) => current.union(box_).map(Some),
            })
            .flatten();
        if let Some(box_) = char_box {
            (Some(box_), Some("char_estimated".to_string()))
        } else if let Some(box_) = self.item.bounding_box {
            (Some(box_), Some("text_item".to_string()))
        } else {
            (None, None)
        }
    }

    #[cfg(test)]
    pub(in crate::text_index) fn candidate_visits(&self) -> usize {
        self.candidate_visits.get()
    }
}

#[cfg(test)]
pub(in crate::text_index) fn match_bounding_box(
    item: &PositionedTextItem,
    start_utf16: u32,
    end_utf16: u32,
) -> (Option<TextBoundingBox>, Option<String>) {
    TextGeometryIndex::new(item).match_bounding_box(start_utf16, end_utf16)
}
