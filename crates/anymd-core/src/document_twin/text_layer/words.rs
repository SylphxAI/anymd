//! Derive whitespace-delimited words and their geometry from positioned text.

use serde_json::{json, Value};

use super::{merge_text_boxes, utf16_len};
use crate::text_index::PositionedTextItem;

pub(super) fn item_words(item: &PositionedTextItem, line_start: u32) -> Vec<Value> {
    let mut words = Vec::new();
    let mut word_start: Option<usize> = None;
    let mut boundaries = item.text.char_indices().collect::<Vec<_>>();
    boundaries.push((item.text.len(), '\0'));
    for (byte_index, character) in boundaries {
        let is_boundary = byte_index == item.text.len() || character.is_whitespace();
        if !is_boundary && word_start.is_none() {
            word_start = Some(byte_index);
        }
        let Some(start) = word_start else {
            continue;
        };
        if !is_boundary {
            continue;
        }
        let text = &item.text[start..byte_index];
        let item_start = utf16_len(&item.text[..start]);
        let item_end = item_start.saturating_add(utf16_len(text));
        let bounding_box = item
            .chars
            .iter()
            .filter(|char_| {
                !char_.is_whitespace
                    && char_.item_char_start >= item_start
                    && char_.item_char_end <= item_end
            })
            .filter_map(|char_| char_.bounding_box)
            .try_fold(None, |current, next| {
                merge_text_boxes(current, next).map(Some).ok_or(())
            })
            .ok()
            .flatten();
        let mut word = json!({
            "index": words.len(),
            "text": text,
            "char_start": line_start.saturating_add(item_start),
            "char_end": line_start.saturating_add(item_end),
        });
        if let Some(box_) = bounding_box {
            word["bounding_box"] = json!(box_);
            word["bounding_box_level"] = json!("char_estimated");
            word["confidence"] = json!(0.74);
        }
        words.push(word);
        word_start = None;
    }
    words
}
