//! Real-world Office samples.
//!
//! `fixtures/equations.docx` and `fixtures/test.xlsx` come from microsoft/markitdown
//! (packages/markitdown/tests/test_files, MIT licence, Copyright (c) Microsoft Corporation).

use anymd_formats::{Format, Options, convert};

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
    assert!(
        first
            .markdown
            .starts_with("|Alpha|Beta|Gamma|Delta|\n|-|-|-|-|\n|89|82|100|12|\n")
    );
    assert!(
        first
            .markdown
            .contains("|58|6ff4173b-42a5-4784-9b19-f49caff4d93d|22|9|")
    );
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

/// Inventory the checked-in non-PDF corpus through the public converter.
#[test]
fn conversion_envelope() {
    use std::collections::BTreeMap;
    use std::io::Read;
    use std::path::{Path, PathBuf};
    fn files(path: &Path, result: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files(&path, result);
            } else {
                result.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    for folder in [
        "test/fixtures",
        "bench/files",
        "crates/anymd-formats/tests/fixtures",
    ] {
        files(&root.join(folder), &mut paths);
    }
    let mut maxima = BTreeMap::new();
    let mut count = 0;
    for path in paths {
        let format = match path.extension().and_then(|s| s.to_str()) {
            Some("docx") => Format::Docx,
            Some("pptx") => Format::Pptx,
            Some("epub") => Format::Epub,
            Some("xlsx") => Format::Xlsx,
            _ => continue,
        };
        let bytes = std::fs::read(&path).unwrap();
        let converted = convert(format, &bytes, &Options::default()).unwrap();
        let output = converted
            .sections
            .iter()
            .map(|s| s.markdown.len())
            .sum::<usize>();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let mut inflated = 0;
        let mut chunk = [0; 16 * 1024];
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            loop {
                let n = entry.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                inflated += n;
            }
        }
        let name = path.strip_prefix(&root).unwrap().display().to_string();
        let maxima = maxima
            .entry(format.name())
            .or_insert(((0, String::new()), (0, String::new())));
        if output > maxima.0.0 {
            maxima.0 = (output, name.clone());
        }
        if inflated > maxima.1.0 {
            maxima.1 = (inflated, name);
        }
        count += 1;
    }
    assert!(count >= 10, "expected the checked-in document corpus");
    for (format, (output, inflated)) in maxima {
        println!(
            "ENVELOPE {format}: output={} file={} inflated={} file={}",
            output.0, output.1, inflated.0, inflated.1
        );
    }
    println!("ENVELOPE documents={count}");
}
