use lopdf::Document;
use serde_json::json;

use super::extract_page_signals;

#[test]
fn tiny_rect_border_width_clamp_matches_pdfjs() {
    let cases = [
        (
            "v3014-annotation-polyline-clamp-w2-v1.pdf",
            "PolyLine",
            json!({"left": 8.0, "bottom": 8.0, "right": 102.0, "top": 82.0}),
        ),
        (
            "v3014-annotation-line-clamp-w2-v1.pdf",
            "Line",
            json!({"left": 7.0, "bottom": 7.0, "right": 103.0, "top": 83.0}),
        ),
        (
            "v3014-annotation-ink-clamp-w2-v1.pdf",
            "Ink",
            json!({"left": 28.0, "bottom": 28.0, "right": 102.0, "top": 92.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load clamp fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_array_width_drives_line_polyline_ink_boxes() {
    let cases = [
        (
            "v3014-annotation-polyline-border-array-w2-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
        (
            "v3014-annotation-line-border-array-w2-v1.pdf",
            "Line",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-ink-border-array-w3-v1.pdf",
            "Ink",
            json!({"left": 24.0, "bottom": 24.0, "right": 106.0, "top": 96.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load border array fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_bs_preference_over_border_array_width() {
    // Border [0 0 9] would expand much more; BS/W must win.
    let cases = [
        (
            "v3014-annotation-polyline-border-bs-pref-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
        (
            "v3014-annotation-line-border-bs-pref-v1.pdf",
            "Line",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-ink-border-bs-pref-v1.pdf",
            "Ink",
            json!({"left": 24.0, "bottom": 24.0, "right": 106.0, "top": 96.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load border BS preference fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_bs_nondict_does_not_fall_through_to_border_array() {
    // BS null with Border [0 0 9] must keep default width 1, not Border[2]=9.
    let cases = [
        (
            "v3014-annotation-polyline-border-bs-null-v1.pdf",
            "PolyLine",
            json!({"left": 8.0, "bottom": 8.0, "right": 102.0, "top": 82.0}),
        ),
        (
            "v3014-annotation-line-border-bs-null-v1.pdf",
            "Line",
            json!({"left": 7.0, "bottom": 7.0, "right": 103.0, "top": 83.0}),
        ),
        (
            "v3014-annotation-ink-border-bs-null-v1.pdf",
            "Ink",
            json!({"left": 28.0, "bottom": 28.0, "right": 102.0, "top": 92.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load border BS nondict fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_array_short_keeps_default_width() {
    // Border length < 3 (no BS) must keep default drawing width 1.
    let cases = [
        (
            "v3014-annotation-polyline-border-short-v1.pdf",
            "PolyLine",
            json!({"left": 8.0, "bottom": 8.0, "right": 102.0, "top": 82.0}),
        ),
        (
            "v3014-annotation-line-border-empty-v1.pdf",
            "Line",
            json!({"left": 7.0, "bottom": 7.0, "right": 103.0, "top": 83.0}),
        ),
        (
            "v3014-annotation-ink-border-short-v1.pdf",
            "Ink",
            json!({"left": 28.0, "bottom": 28.0, "right": 102.0, "top": 92.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load short Border fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_bs_wrong_type_ignores_w_and_border() {
    // BS Type not Border with W=9 and Border[2]=5 must keep default width 1.
    let cases = [
        (
            "v3014-annotation-polyline-border-bs-wrong-type-v1.pdf",
            "PolyLine",
            json!({"left": 8.0, "bottom": 8.0, "right": 102.0, "top": 82.0}),
        ),
        (
            "v3014-annotation-line-border-bs-wrong-type-v1.pdf",
            "Line",
            json!({"left": 7.0, "bottom": 7.0, "right": 103.0, "top": 83.0}),
        ),
        (
            "v3014-annotation-ink-border-bs-wrong-type-v1.pdf",
            "Ink",
            json!({"left": 28.0, "bottom": 28.0, "right": 102.0, "top": 92.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load BS wrong-type fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_zero_size_rect_bypasses_width_clamp() {
    // Zero-dimension Rect + BS/W=2 must keep width 2 (no clamp).
    let cases = [
        (
            "v3014-annotation-polyline-zero-h-w2-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
        (
            "v3014-annotation-line-zero-h-w2-v1.pdf",
            "Line",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-ink-zero-w-w2-v1.pdf",
            "Ink",
            json!({"left": 26.0, "bottom": 26.0, "right": 104.0, "top": 94.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load zero-size clamp-bypass fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}
