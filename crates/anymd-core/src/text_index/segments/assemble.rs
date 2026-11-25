//! Assemble positioned segments with inferred spaces, UTF-16 offsets, and source runs.

use super::super::{PositionedTextItem, PositionedTextRun, TextCharacterGeometry, TextIndexError};
use super::RawTextPart;

fn offset_overflow() -> TextIndexError {
    TextIndexError::extraction_failed("selectable text offset overflow")
}

/// For the characters of one segment in reading order, whether a synthetic
/// word space belongs before each one.
///
/// pdf-extract reports each show-text string as its own part and never emits
/// the word spaces that TeX and similar producers express only as glyph
/// positioning, so the text would read "Thedominantsequence...". The gaps
/// between glyphs decide instead, with the same rules as the anymd-pdf
/// layout engine behind the Markdown path.
fn segment_word_spaces(parts: &[RawTextPart]) -> Vec<bool> {
    let glyphs = parts
        .iter()
        .flat_map(|part| {
            part.item
                .chars
                .iter()
                .enumerate()
                .map(|(index, character)| {
                    let extent = part.glyphs.get(index).copied().flatten();
                    anymd_pdf::SpacingGlyph {
                        x0: extent.map_or(f64::NAN, |extent| extent.x0),
                        x1: extent.map_or(f64::NAN, |extent| extent.x1),
                        size: extent.map_or(f64::NAN, |extent| extent.size),
                        text: character.text.as_str(),
                    }
                })
        })
        .collect::<Vec<_>>();
    anymd_pdf::infer_word_spaces(&glyphs)
}

pub(super) fn merge_raw_text_segment(
    parts: Vec<RawTextPart>,
) -> Result<PositionedTextItem, TextIndexError> {
    let spaces_before = segment_word_spaces(&parts);
    let mut spaces_before = spaces_before.into_iter();
    let mut text = String::new();
    let mut chars = Vec::new();
    let mut runs: Vec<PositionedTextRun> = Vec::with_capacity(parts.len());
    let mut bounding_box = None;
    let mut offset = 0u32;

    for part in parts {
        let mut run_start = offset;
        let mut run_text = String::with_capacity(part.item.text.len());
        for (index, mut character) in part.item.chars.into_iter().enumerate() {
            if spaces_before.next().unwrap_or(false) {
                // A space before a part's first glyph ends the previous run,
                // so runs stay contiguous and each keeps its own source text.
                let space_start = offset;
                offset = offset.checked_add(1).ok_or_else(offset_overflow)?;
                text.push(' ');
                chars.push(TextCharacterGeometry {
                    text: " ".to_string(),
                    item_char_start: space_start,
                    item_char_end: offset,
                    is_whitespace: true,
                    bounding_box: None,
                });
                match runs.last_mut() {
                    Some(previous) if index == 0 => {
                        previous.text.push(' ');
                        previous.item_char_end = offset;
                        run_start = offset;
                    }
                    _ => run_text.push(' '),
                }
            }
            let len = character
                .item_char_end
                .checked_sub(character.item_char_start)
                .ok_or_else(|| {
                    TextIndexError::extraction_failed("selectable text character offset overflow")
                })?;
            character.item_char_start = offset;
            offset = offset.checked_add(len).ok_or_else(offset_overflow)?;
            character.item_char_end = offset;
            text.push_str(&character.text);
            run_text.push_str(&character.text);
            chars.push(character);
        }
        if let Some(box_) = part.item.bounding_box {
            bounding_box = match bounding_box {
                None => Some(box_),
                Some(current) => Some(current.union(box_).ok_or_else(|| {
                    TextIndexError::extraction_failed("selectable text bounding-box overflow")
                })?),
            };
        }
        runs.push(PositionedTextRun {
            text: run_text,
            item_char_start: run_start,
            item_char_end: offset,
            bounding_box: part.item.bounding_box,
        });
    }

    Ok(PositionedTextItem {
        text,
        bounding_box,
        chars,
        runs,
    })
}
