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
    assert!(first
        .markdown
        .starts_with("|Alpha|Beta|Gamma|Delta|\n|-|-|-|-|\n|89|82|100|12|\n"));
    assert!(first
        .markdown
        .contains("|58|6ff4173b-42a5-4784-9b19-f49caff4d93d|22|9|"));
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
        if output > maxima.0 .0 {
            maxima.0 = (output, name.clone());
        }
        if inflated > maxima.1 .0 {
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

/// A complete synthetic XLSB built from MS-XLSB records (no opaque fixture).
/// Distinct sheet names deliberately reopen the very same binary ZIP target.
fn aliased_xlsb(aliases: usize, padding: usize, uppercase: bool) -> Vec<u8> {
    use std::io::{Cursor, Write};
    fn wide(text: &str) -> Vec<u8> {
        let mut out = (text.encode_utf16().count() as u32).to_le_bytes().to_vec();
        for unit in text.encode_utf16() {
            out.extend(unit.to_le_bytes());
        }
        out
    }
    fn record(out: &mut Vec<u8>, kind: u16, data: &[u8]) {
        if kind >= 128 {
            out.push((kind as u8 & 127) | 128);
            out.push((kind >> 7) as u8);
        } else {
            out.push(kind as u8);
        }
        let mut size = data.len();
        loop {
            let byte = (size & 127) as u8;
            size >>= 7;
            out.push(byte | if size > 0 { 128 } else { 0 });
            if size == 0 {
                break;
            }
        }
        out.extend_from_slice(data);
    }
    let mut book = Vec::new();
    for index in 0..aliases {
        let mut sheet = vec![0; 4]; // visible
        sheet.extend(((index + 1) as u32).to_le_bytes());
        sheet.extend(wide("rId1"));
        sheet.extend(wide(&format!("Sheet{}", index + 1)));
        record(&mut book, 0x009c, &sheet); // BrtBundleSh
    }
    record(&mut book, 0x0090, &[]); // BrtEndBundleShs
    record(&mut book, 0x0084, &[]); // BrtEndBook
    let mut sheet = Vec::new();
    record(&mut sheet, 0x0094, &[0; 16]); // BrtWsDim
    record(&mut sheet, 0x0091, &[]); // BrtBeginSheetData
    record(&mut sheet, 0x01ff, &vec![0; padding]); // ignored record, still inflated
    record(&mut sheet, 0x0092, &[]); // BrtEndSheetData
    let workbook = if uppercase {
        "xl/workbook.BIN"
    } else {
        "xl/workbook.bin"
    };
    let worksheet = if uppercase {
        "xl/worksheets/sheet1.BIN"
    } else {
        "xl/worksheets/sheet1.bin"
    };
    let root = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="office" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="{workbook}"/></Relationships>"#
    );
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="{}"/></Relationships>"#,
        worksheet.strip_prefix("xl/").unwrap()
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("_rels/.rels", root.as_bytes()),
        ("xl/_rels/workbook.bin.rels", rels.as_bytes()),
        (workbook, book.as_slice()),
        (worksheet, sheet.as_slice()),
    ] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn benign_binary_workbooks_and_ods_still_convert() {
    use std::io::{Cursor, Write};
    for uppercase in [false, true] {
        let book = convert(
            Format::Xlsx,
            &aliased_xlsb(2, 1024, uppercase),
            &Options::default(),
        )
        .unwrap();
        assert_eq!(book.sections.len(), 2);
        assert!(book
            .sections
            .iter()
            .all(|section| section.markdown == "(empty sheet)\n"));
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
        (
            "META-INF/manifest.xml",
            r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#,
        ),
        (
            "content.xml",
            r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><table:table table:name="Revenue"><table:table-row><table:table-cell office:value-type="string"><text:p>Sample</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#,
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(value.as_bytes()).unwrap();
    }
    let out = convert(
        Format::Xlsx,
        &zip.finish().unwrap().into_inner(),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(out.sections[0].markdown, "## Revenue\n\n|Sample|\n|-|\n");
}

#[test]
fn aliased_binary_worksheet_cannot_escape_the_public_inflation_ceiling() {
    // Unique inflation is ~1 MiB and Markdown is <7 KiB; the only exhausted
    // resource is cumulative decompression of that worksheet's repeated reads.
    // Preflight and format detection also consume the same 512 MiB budget.
    for uppercase in [false, true] {
        let bytes = aliased_xlsb(513, 1024 * 1024, uppercase);
        let error = convert(Format::Xlsx, &bytes, &Options::default()).unwrap_err();
        assert_eq!(
            error.limit_exceeded(),
            Some(anymd_formats::ConversionLimit {
                resource: "inflated input",
                limit: 512 * 1024 * 1024
            })
        );
    }
}
