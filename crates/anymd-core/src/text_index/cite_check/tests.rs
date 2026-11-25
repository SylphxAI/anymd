mod geometry;

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
