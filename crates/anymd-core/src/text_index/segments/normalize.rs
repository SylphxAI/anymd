//! Group raw text by row and horizontal gaps in deterministic reading order.

use std::collections::BTreeMap;

use super::super::{
    PositionedTextItem, TextIndexError, MAX_NORMALIZED_TEXT_SEGMENTS,
    MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE, TEXT_SEGMENT_GAP_THRESHOLD,
};
use super::{assemble::merge_raw_text_segment, RawTextPart};

fn normalized_row_key(y: f64) -> Result<i64, TextIndexError> {
    if !y.is_finite() {
        return Err(TextIndexError::extraction_failed(
            "selectable text contains a non-finite row coordinate",
        ));
    }
    // JavaScript Math.round(x) is floor(x + 0.5), including for negative halves.
    let rounded = (y + 0.5).floor();
    if rounded < i64::MIN as f64 || rounded > i64::MAX as f64 {
        return Err(TextIndexError::extraction_failed(
            "selectable text row coordinate exceeds the supported range",
        ));
    }
    Ok(rounded as i64)
}

pub(in crate::text_index) fn normalize_page_text_parts(
    parts: Vec<RawTextPart>,
    request_segment_count: &mut usize,
) -> Result<Vec<PositionedTextItem>, TextIndexError> {
    let mut rows = BTreeMap::<i64, Vec<(usize, RawTextPart)>>::new();
    for (source_index, part) in parts.into_iter().enumerate() {
        let x = part.x.filter(|value| value.is_finite()).ok_or_else(|| {
            TextIndexError::extraction_failed("selectable text contains an invalid X coordinate")
        })?;
        let y = part.y.ok_or_else(|| {
            TextIndexError::extraction_failed("selectable text contains a missing Y coordinate")
        })?;
        let right = part
            .right
            .or_else(|| part.item.bounding_box.map(|box_| box_.right))
            .filter(|value| value.is_finite() && *value >= x)
            .ok_or_else(|| {
                TextIndexError::extraction_failed(
                    "selectable text contains an invalid horizontal advance",
                )
            })?;
        let key = normalized_row_key(y)?;
        let mut part = part;
        part.x = Some(x);
        part.right = Some(right);
        rows.entry(key).or_default().push((source_index, part));
    }

    let mut output = Vec::new();
    for (_, mut row) in rows.into_iter().rev() {
        row.sort_by(|(left_index, left), (right_index, right)| {
            left.x
                .unwrap_or_default()
                .total_cmp(&right.x.unwrap_or_default())
                .then_with(|| left_index.cmp(right_index))
        });
        let mut current = Vec::new();
        let mut previous_right: Option<f64> = None;
        for (_, part) in row {
            let x = part.x.expect("validated text-part X");
            if previous_right.is_some_and(|right| x - right > TEXT_SEGMENT_GAP_THRESHOLD) {
                if output.len() >= MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE
                    || *request_segment_count >= MAX_NORMALIZED_TEXT_SEGMENTS
                {
                    return Err(TextIndexError::extraction_failed(
                        "selectable text exceeds bounded normalized-segment budget",
                    ));
                }
                output.push(merge_raw_text_segment(std::mem::take(&mut current))?);
                *request_segment_count += 1;
            }
            previous_right = Some(previous_right.map_or(
                part.right.expect("validated text-part right edge"),
                |right| right.max(part.right.expect("validated text-part right edge")),
            ));
            current.push(part);
        }
        if !current.is_empty() {
            if output.len() >= MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE
                || *request_segment_count >= MAX_NORMALIZED_TEXT_SEGMENTS
            {
                return Err(TextIndexError::extraction_failed(
                    "selectable text exceeds bounded normalized-segment budget",
                ));
            }
            output.push(merge_raw_text_segment(current)?);
            *request_segment_count += 1;
        }
    }
    Ok(output)
}
