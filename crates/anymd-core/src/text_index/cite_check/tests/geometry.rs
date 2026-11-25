use super::*;

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
