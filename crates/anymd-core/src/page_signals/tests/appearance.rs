use lopdf::Document;
use serde_json::json;

use super::extract_page_signals;

#[test]
fn appearance_present_skips_line_polyline_ink_geometry_expansion() {
    // AP/N present => keep raw Rect even when L/vertices would expand farther.
    let cases = [
        (
            "v3014-annotation-line-ap-bbox-v1.pdf",
            "Line",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-polyline-ap-bbox-v1.pdf",
            "PolyLine",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-ink-ap-bbox-v1.pdf",
            "Ink",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load appearance bbox fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ap_n_nonstream_still_expands_line_polyline_ink_geometry() {
    // AP/N null or name is not appearance => expand geometry with BS/W=2.
    let cases = [
        (
            "v3014-annotation-line-ap-n-null-v1.pdf",
            "Line",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-polyline-ap-n-name-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
        (
            "v3014-annotation-ink-ap-n-null-v1.pdf",
            "Ink",
            json!({"left": 26.0, "bottom": 26.0, "right": 104.0, "top": 94.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load AP non-stream fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ap_named_state_as_selection_for_line_geometry() {
    let cases = [
        (
            "v3014-annotation-line-ap-as-on-v1.pdf",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-line-ap-as-missing-v1.pdf",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-line-ap-as-invalid-v1.pdf",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
    ];
    for (fixture, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load AP named-state fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], "Line", "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ap_named_state_polyline_ink_breadth() {
    let cases = [
        (
            "v3014-annotation-polyline-ap-as-on-v1.pdf",
            "PolyLine",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-ink-ap-as-missing-v1.pdf",
            "Ink",
            json!({"left": 26.0, "bottom": 26.0, "right": 104.0, "top": 94.0}),
        ),
        (
            "v3014-annotation-polyline-ap-as-invalid-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load named-state polyline/ink fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ap_named_state_square_circle_breadth() {
    let cases = [
        (
            "v3014-annotation-square-ap-as-on-v1.pdf",
            "Square",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-circle-ap-as-missing-v1.pdf",
            "Circle",
            json!({"left": 50.0, "bottom": 60.0, "right": 150.0, "top": 160.0}),
        ),
        (
            "v3014-annotation-square-ap-as-invalid-v1.pdf",
            "Square",
            json!({"left": 10.0, "bottom": 20.0, "right": 110.0, "top": 120.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load named-state square/circle fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}
