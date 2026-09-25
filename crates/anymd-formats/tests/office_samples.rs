//! Real-world Office samples.
//!
//! `fixtures/equations.docx` and `fixtures/test.xlsx` come from microsoft/markitdown
//! (packages/markitdown/tests/test_files, MIT licence, Copyright (c) Microsoft Corporation).

use anymd_formats::{convert, Format, Options};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn word_equations_become_latex() {
    let out = convert(
        Format::Docx,
        &fixture("equations.docx"),
        &Options::default(),
    )
    .unwrap();
    let markdown = &out.sections[0].markdown;
    assert!(markdown.starts_with("For $m=1$,\n\n$$"), "{markdown}");
    assert!(markdown.contains(r"\frac{mλ}{a}"), "{markdown}");
    assert!(markdown.contains(r"{10}^{-6}"), "{markdown}");
    assert!(out.metadata.iter().any(|(k, _)| k == "author"));
}

#[test]
fn spreadsheet_sample_renders_header_and_rows() {
    let out = convert(Format::Xlsx, &fixture("test.xlsx"), &Options::default()).unwrap();
    let first = &out.sections[0];
    assert_eq!(first.label, "sheet Sheet1");
    assert!(first.markdown.starts_with(
        "| Alpha | Beta | Gamma | Delta |\n| --- | --- | --- | --- |\n| 89 | 82 | 100 | 12 |\n"
    ));
    assert!(first
        .markdown
        .contains("| 58 | 6ff4173b-42a5-4784-9b19-f49caff4d93d | 22 | 9 |"));
    for section in &out.sections {
        assert!(
            !section.markdown.contains(".0 |"),
            "integers must not carry .0: {}",
            section.markdown
        );
    }
}

#[test]
fn corrupted_samples_never_panic() {
    for (format, name) in [
        (Format::Docx, "equations.docx"),
        (Format::Xlsx, "test.xlsx"),
        (Format::Pptx, "equations.docx"),
    ] {
        let original = fixture(name);
        for cut in (0..original.len()).step_by(997) {
            let _ = convert(format, &original[..cut], &Options::default());
            let mut flipped = original.clone();
            flipped[cut] ^= 0x5a;
            let _ = convert(format, &flipped, &Options::default());
        }
    }
}
