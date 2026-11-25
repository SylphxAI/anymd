use lopdf::{dictionary, Document, Object};
use serde_json::json;

use super::{document_with_pages, extract_page_signals};

#[test]
fn text_annotation_without_appearance_uses_pdfjs_icon_box() {
    let annotation = Object::Dictionary(dictionary! {
        "Subtype" => "Text",
        "Contents" => "Sticky note",
        "T" => "Author",
        "Rect" => vec![120.into(), 680.into(), 140.into(), 700.into()],
    });
    let document = document_with_pages(vec![dictionary! {
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![annotation],
    }]);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["bounding_box"],
        json!({"left": 120.0, "bottom": 678.0, "right": 142.0, "top": 700.0})
    );
    assert_eq!(signals.annotations[0]["annotations"][0]["subtype"], "Text");
}

#[test]
fn freetext_annotation_keeps_raw_rect_box() {
    let annotation = Object::Dictionary(dictionary! {
        "Subtype" => "FreeText",
        "Contents" => "Hello FreeText",
        "Rect" => vec![100.into(), 600.into(), 250.into(), 650.into()],
    });
    let document = document_with_pages(vec![dictionary! {
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![annotation],
    }]);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["bounding_box"],
        json!({"left": 100.0, "bottom": 600.0, "right": 250.0, "top": 650.0})
    );
}

#[test]
fn popup_zero_size_rect_omits_bounding_box() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-popup-zerosize-v1.pdf");
    if !path.is_file() {
        return;
    }
    let document = Document::load(&path).expect("load zero-size popup fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let anns = signals.annotations[0]["annotations"].as_array().unwrap();
    let popup = anns.iter().find(|a| a["subtype"] == "Popup").unwrap();
    assert!(
        popup.get("bounding_box").is_none(),
        "zero-size popup must omit bounding_box"
    );
    assert_eq!(popup["contents"], "Parent note");
    assert_eq!(popup["title"], "Author");
    let text = anns.iter().find(|a| a["subtype"] == "Text").unwrap();
    assert_eq!(
        text["bounding_box"],
        json!({"left": 100.0, "bottom": 650.0, "right": 122.0, "top": 672.0})
    );
}

#[test]
fn text_with_appearance_keeps_raw_rect_even_if_stream_empty() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-ap-v1.pdf");
    if path.is_file() {
        let document = Document::load(&path).expect("load text ap fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], "Text");
        assert_eq!(
            ann["bounding_box"],
            json!({"left": 100.0, "bottom": 600.0, "right": 200.0, "top": 700.0})
        );
    }
    let empty = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-emptyap-v1.pdf");
    if empty.is_file() {
        let document = Document::load(&empty).expect("load empty ap fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], "Text");
        assert_eq!(
            ann["bounding_box"],
            json!({"left": 50.0, "bottom": 600.0, "right": 150.0, "top": 700.0})
        );
    }
}

#[test]
fn text_named_appearance_requires_as_for_raw_rect() {
    let with_as = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-namedap-v1.pdf");
    if with_as.is_file() {
        let document = Document::load(&with_as).expect("load named ap fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(
            ann["bounding_box"],
            json!({"left": 80.0, "bottom": 600.0, "right": 180.0, "top": 700.0})
        );
    }
    let no_as = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-namedap-noas-v1.pdf");
    if no_as.is_file() {
        let document = Document::load(&no_as).expect("load named ap no-as fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        // pdf.js icon box: bottom = top-22, right = left+22
        assert_eq!(
            ann["bounding_box"],
            json!({"left": 90.0, "bottom": 688.0, "right": 112.0, "top": 710.0})
        );
    }
}

#[test]
fn text_no_appearance_normalizes_inverted_rect_before_icon_box() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-inverted-v1.pdf");
    if !path.is_file() {
        return;
    }
    let document = Document::load(&path).expect("load inverted text fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let ann = &signals.annotations[0]["annotations"][0];
    assert_eq!(ann["subtype"], "Text");
    assert_eq!(ann["contents"], "Inverted");
    assert_eq!(
        ann["bounding_box"],
        json!({"left": 100.0, "bottom": 678.0, "right": 122.0, "top": 700.0})
    );
}

#[test]
fn polyline_polygon_nonintersecting_rect_uses_vertices_bbox() {
    let cases = [
        (
            "v3014-annotation-polyline-l-bbox-v1.pdf",
            "PolyLine",
            json!({"left": 98.0, "bottom": 98.0, "right": 202.0, "top": 202.0}),
        ),
        (
            "v3014-annotation-polygon-l-bbox-v1.pdf",
            "Polygon",
            json!({"left": 48.0, "bottom": 48.0, "right": 122.0, "top": 122.0}),
        ),
        (
            "v3014-annotation-polyline-border2-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load polyline/polygon fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype);
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ink_nonintersecting_rect_uses_inklist_bbox() {
    let cases = [
        (
            "v3014-annotation-ink-l-bbox-v1.pdf",
            json!({"left": 98.0, "bottom": 98.0, "right": 182.0, "top": 202.0}),
        ),
        (
            "v3014-annotation-ink-multistroke-v1.pdf",
            json!({"left": 8.0, "bottom": 48.0, "right": 92.0, "top": 122.0}),
        ),
        (
            "v3014-annotation-ink-border2-v1.pdf",
            json!({"left": 26.0, "bottom": 26.0, "right": 104.0, "top": 94.0}),
        ),
    ];
    for (fixture, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load ink fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], "Ink");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn highlight_quadpoints_bbox() {
    let cases = [
        (
            "v3014-annotation-highlight-quad-noap-v1.pdf",
            "Highlight",
            json!({"left": 10.0, "bottom": 10.0, "right": 100.0, "top": 80.0}),
        ),
        (
            "v3014-annotation-highlight-ap-noext-v1.pdf",
            "Highlight",
            json!({"left": 10.0, "bottom": 10.0, "right": 100.0, "top": 80.0}),
        ),
        (
            "v3014-annotation-highlight-ap-ext-v1.pdf",
            "Highlight",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load highlight fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn text_markup_quadpoints_breadth() {
    let cases = [
        (
            "v3014-annotation-underline-quad-noap-v1.pdf",
            "Underline",
            json!({"left": 20.0, "bottom": 20.0, "right": 120.0, "top": 90.0}),
        ),
        (
            "v3014-annotation-squiggly-quad-noap-v1.pdf",
            "Squiggly",
            json!({"left": 30.0, "bottom": 6.666666666666668, "right": 130.0, "top": 53.33333333333333}),
        ),
        (
            "v3014-annotation-strikeout-quad-noap-v1.pdf",
            "StrikeOut",
            json!({"left": 40.0, "bottom": 40.0, "right": 140.0, "top": 110.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load text-markup fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn text_markup_with_appearance_keeps_rect() {
    let cases = [
        (
            "v3014-annotation-underline-ap-keeps-rect-v1.pdf",
            "Underline",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-squiggly-ap-keeps-rect-v1.pdf",
            "Squiggly",
            json!({"left": 150.0, "bottom": 160.0, "right": 250.0, "top": 260.0}),
        ),
        (
            "v3014-annotation-strikeout-ap-keeps-rect-v1.pdf",
            "StrikeOut",
            json!({"left": 100.0, "bottom": 110.0, "right": 210.0, "top": 220.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load text-markup AP fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}
