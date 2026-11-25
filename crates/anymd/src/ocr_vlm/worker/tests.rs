use super::*;

#[test]
fn table_html_and_formulas_use_the_existing_markdown_converter() {
    use anymd_ocr_vlm::{PageResult, Region};
    let region = |label: &str, text: &str| Region {
        label: label.into(),
        bbox: [0., 0., 10., 10.],
        score: 0.9,
        order: 1,
        text: text.into(),
    };
    let mut result = PageResult {
        regions: vec![
            region(
                "table",
                "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
            ),
            region("display_formula", "x^2 + y^2"),
        ],
        truncated: 0,
    };
    format_regions(&mut result).unwrap();
    assert_eq!(result.regions[0].text, "|A|B|\n|-|-|\n|1|2|");
    assert_eq!(result.regions[1].text, "$$\nx^2 + y^2\n$$");
}

/// The companion protocol with a stub engine: argv carries the worker
/// name, the page-image path and the token cap; stdout carries the JSON.
#[cfg(unix)]
#[test]
fn stub_companion_round_trips_a_page() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("anymd-ocr-vlm");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
[ "$1" = "__ocr-vlm-worker" ] || exit 3
[ "$(cat "$2")" = "page-bytes" ] || exit 4
[ "$3" = "17" ] || exit 5
printf '{"regions":[{"label":"text","bbox":[0,0,1,1],"score":0.9,"order":2,"text":"world"},{"label":"text","bbox":[0,0,1,1],"score":0.9,"order":1,"text":"hello"}],"truncated":0,"device":"cpu"}'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let page = run_worker(&stub, b"page-bytes", 10_000, 17).unwrap();
    assert_eq!(page.markdown(), "hello\n\nworld");
    // A worker that exits non-zero is an error, not a page.
    assert!(run_worker(&stub, b"other-bytes", 10_000, 17).is_err());
}
