//! Office conversion parity across ZIP entry encodings.

use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{Format, Options};

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
