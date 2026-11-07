//! Behavioral checks for stricter archive entry validation.

use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::ooxml::{guard_zip, Package};
use crate::{ConvertError, Format, Options};

fn archive(streamed: bool, large: bool, method: CompressionMethod) -> Vec<u8> {
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .large_file(large);
    let parts = [("first", b"first payload"), ("other", b"other payload")];
    if streamed {
        let mut zip = ZipWriter::new_stream(Vec::new());
        for (name, data) in parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    } else {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
}

fn offsets(bytes: &[u8]) -> Vec<(usize, usize, usize)> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|index| {
            let file = zip.by_index_raw(index).unwrap();
            (
                file.header_start() as usize,
                file.central_header_start() as usize,
                file.data_start().unwrap() as usize,
            )
        })
        .collect()
}

fn refused(bytes: &[u8]) {
    let error = guard_zip(bytes).expect_err("malformed container must be refused");
    assert!(matches!(error, ConvertError::Invalid(_)), "{error:?}");
    // Every direct ZIP-based format entrypoint validates before optional parts
    // or a third-party parser can swallow the malformed entry.
    for format in [Format::Docx, Format::Xlsx, Format::Pptx, Format::Epub] {
        assert_eq!(
            crate::convert(format, bytes, &Options::default()),
            Err(error.clone())
        );
    }
}

#[test]
fn shared_local_header_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    let entries = offsets(&bytes);
    let first = (entries[0].0 as u32).to_le_bytes();
    bytes[entries[1].1 + 42..entries[1].1 + 46].copy_from_slice(&first);
    refused(&bytes);
}

#[test]
fn overlapping_payload_ranges_are_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    let entries = offsets(&bytes);
    // Keep the first local and central metadata consistent while extending
    // its stored payload through the next entry's independently valid payload.
    let length = (entries[1].2 + b"other payload".len() - entries[0].2) as u32;
    for start in [entries[0].0 + 18, entries[0].1 + 20] {
        bytes[start..start + 4].copy_from_slice(&length.to_le_bytes());
        bytes[start + 4..start + 8].copy_from_slice(&length.to_le_bytes());
    }
    refused(&bytes);
}

#[test]
fn local_name_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[30] = b'x';
    refused(&bytes);
}

#[test]
fn local_method_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[8] = 8;
    refused(&bytes);
}

#[test]
fn local_flags_mismatch_is_refused() {
    for bit in 0..16 {
        let mut bytes = archive(false, false, CompressionMethod::Stored);
        let flags = (1u16 << bit).to_le_bytes();
        bytes[6..8].copy_from_slice(&flags);
        refused(&bytes);
    }
}

#[test]
fn local_crc_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[14] ^= 1;
    refused(&bytes);
}

#[test]
fn local_compressed_size_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[18] ^= 1;
    refused(&bytes);
}

#[test]
fn local_inflated_size_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[22] ^= 1;
    refused(&bytes);
}

#[test]
fn descriptor_nonzero_crc_mismatch_is_refused() {
    let mut bytes = archive(true, false, CompressionMethod::Deflated);
    bytes[14..18].copy_from_slice(&1u32.to_le_bytes());
    refused(&bytes);
}

#[test]
fn local_zip64_size_mismatch_is_refused() {
    let mut bytes = archive(false, true, CompressionMethod::Stored);
    // The writer reserves ZIP64 sizes in the local extra field.
    let extra = 30 + "first".len();
    assert_eq!(&bytes[extra..extra + 2], &1u16.to_le_bytes());
    bytes[extra + 4] ^= 1;
    refused(&bytes);
}

#[test]
fn stored_deflated_descriptor_and_zip64_entries_remain_readable() {
    for streamed in [false, true] {
        for large in [false, true] {
            for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
                let bytes = archive(streamed, large, method);
                guard_zip(&bytes).unwrap();
                let mut package = Package::open(&bytes, "test").unwrap();
                let entries = package.preflight().unwrap();
                assert_eq!(entries.len(), 2);
                for (index, entry) in entries.iter().enumerate() {
                    assert_eq!(entry.index, index);
                    assert_eq!(entry.inflated, b"first payload".len());
                }
                assert_eq!(package.read("first").unwrap().unwrap(), b"first payload");
                assert_eq!(package.read("other").unwrap().unwrap(), b"other payload");
            }
        }
    }
}

#[test]
fn timestamps_are_not_entry_identity() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    // Some ordinary writers record different timestamps in the two headers.
    bytes[10..14].fill(0);
    guard_zip(&bytes).unwrap();
    let mut package = Package::open(&bytes, "test").unwrap();
    assert_eq!(package.read("first").unwrap().unwrap(), b"first payload");
}

#[test]
fn preflight_remeasures_each_index_and_keeps_the_cumulative_charge() {
    let bytes = archive(false, false, CompressionMethod::Deflated);
    let mut package = Package::open(&bytes, "test").unwrap();
    package.limits = crate::limits::Limits::with_inflated_limit(51);
    let entries = package.preflight().unwrap();
    assert_eq!(
        entries.iter().map(|entry| entry.inflated).sum::<usize>(),
        26
    );
    assert_eq!(
        package.preflight().unwrap_err(),
        ConvertError::limit("inflated input", 51)
    );
}

fn repack(bytes: &[u8], streamed: bool, large: bool, method: CompressionMethod) -> Vec<u8> {
    let mut source = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .large_file(large);
    let mut parts = Vec::new();
    for index in 0..source.len() {
        let mut file = source.by_index(index).unwrap();
        let name = file.name().unwrap().into_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        parts.push((name, data));
    }
    if streamed {
        let mut zip = ZipWriter::new_stream(Vec::new());
        for (name, data) in parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(&data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    } else {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(&data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
}

fn zip_parts(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in parts {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

// Small MS-XLSB records, generated here rather than committing a workbook.
fn binary_workbook() -> Vec<u8> {
    fn record(out: &mut Vec<u8>, kind: u16, data: &[u8]) {
        if kind >= 0x80 {
            out.extend([(kind as u8 & 0x7f) | 0x80, (kind >> 7) as u8]);
        } else {
            out.push(kind as u8);
        }
        assert!(data.len() < 128);
        out.push(data.len() as u8);
        out.extend(data);
    }
    fn wide(out: &mut Vec<u8>, text: &str) {
        out.extend((text.encode_utf16().count() as u32).to_le_bytes());
        for unit in text.encode_utf16() {
            out.extend(unit.to_le_bytes());
        }
    }
    let mut book = Vec::new();
    record(&mut book, 0x83, &[]); // BrtBeginBook
    record(&mut book, 0x8f, &[]); // BrtBeginBundleShs
    let mut sheet = vec![0; 4]; // visible
    sheet.extend(1u32.to_le_bytes());
    wide(&mut sheet, "rId1");
    wide(&mut sheet, "Sheet1");
    record(&mut book, 0x9c, &sheet); // BrtBundleSh
    record(&mut book, 0x90, &[]); // BrtEndBundleShs
    record(&mut book, 0x84, &[]); // BrtEndBook
    let mut cells = Vec::new();
    record(&mut cells, 0x81, &[]); // BrtBeginSheet
    record(&mut cells, 0x94, &[0; 16]); // BrtWsDim
    record(&mut cells, 0x91, &[]); // BrtBeginSheetData
    record(&mut cells, 0, &[0; 4]); // BrtRowHdr
    let mut cell = vec![0; 8];
    wide(&mut cell, "sample");
    record(&mut cells, 6, &cell); // BrtCellSt
    record(&mut cells, 0x92, &[]); // BrtEndSheetData
    record(&mut cells, 0x82, &[]); // BrtEndSheet
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("xl/workbook.bin", book.as_slice()),
        ("xl/worksheets/sheet1.bin", cells.as_slice()),
        ("xl/_rels/workbook.bin.rels", b"<Relationships><Relationship Id='rId1' Target='worksheets/sheet1.bin'/></Relationships>".as_slice()),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn office_conversion_is_unchanged_across_container_encodings() {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    workbook
        .add_worksheet()
        .write_string(0, 0, "sample")
        .unwrap();
    let xlsx = workbook.save_to_buffer().unwrap();
    let xlsm = {
        let mut source = ZipArchive::new(Cursor::new(&xlsx)).unwrap();
        let mut target = ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..source.len() {
            let mut file = source.by_index(index).unwrap();
            let name = file.name().unwrap().into_owned();
            let mut text = String::new();
            file.read_to_string(&mut text).unwrap();
            if name == "[Content_Types].xml" {
                text = text.replace(
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
                    "application/vnd.ms-excel.sheet.macroEnabled.main+xml",
                );
            }
            target
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            target.write_all(text.as_bytes()).unwrap();
        }
        target.finish().unwrap().into_inner()
    };
    let xlsb = binary_workbook();
    let docx = zip_parts(&[("word/document.xml", "<w:document xmlns:w='w'><w:body><w:p><w:r><w:t>sample</w:t></w:r></w:p></w:body></w:document>")]);
    let pptx = zip_parts(&[
        ("ppt/presentation.xml", "<p:presentation xmlns:p='p' xmlns:r='r'><p:sldIdLst><p:sldId r:id='rId1'/></p:sldIdLst></p:presentation>"),
        ("ppt/_rels/presentation.xml.rels", "<Relationships><Relationship Id='rId1' Type='slide' Target='slides/slide1.xml'/></Relationships>"),
        ("ppt/slides/slide1.xml", "<p:sld xmlns:p='p' xmlns:a='a'><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>sample</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"),
    ]);
    for (format, bytes) in [
        (Format::Docx, docx),
        (Format::Xlsx, xlsx),
        (Format::Xlsx, xlsm),
        (Format::Xlsx, xlsb),
        (Format::Pptx, pptx),
    ] {
        let expected = crate::convert(format, &bytes, &Options::default()).unwrap();
        assert!(expected
            .sections
            .iter()
            .any(|section| section.markdown.contains("sample")));
        for streamed in [false, true] {
            for large in [false, true] {
                for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
                    let encoded = repack(&bytes, streamed, large, method);
                    assert_eq!(
                        crate::convert(format, &encoded, &Options::default()).unwrap(),
                        expected
                    );
                }
            }
        }
    }
}
