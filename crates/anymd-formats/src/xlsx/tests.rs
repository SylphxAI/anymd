use std::cell::RefCell;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::rc::Rc;

use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

use super::{convert, input::AccountedInput, render::number, workbook::convert_with_limits};
use crate::limits::Limits;
use crate::ooxml::EntryInflation;
use crate::{ConvertError, Options};

fn workbook() -> Vec<u8> {
    let mut workbook = Workbook::new();
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let sheet = workbook.add_worksheet().set_name("Revenue").unwrap();
    sheet.write_string(1, 1, "Region").unwrap();
    sheet.write_string(1, 2, "Amount").unwrap();
    sheet.write_string(1, 3, "Day").unwrap();
    sheet.write_string(2, 1, "EU|West").unwrap();
    sheet.write_number(2, 2, 1200.0).unwrap();
    sheet
        .write_datetime_with_format(2, 3, ExcelDateTime::from_ymd(2024, 3, 9).unwrap(), &date)
        .unwrap();
    sheet.write_string(4, 1, "US").unwrap();
    sheet.write_number(4, 2, 0.1 + 0.2).unwrap();
    sheet.write_boolean(4, 3, true).unwrap();
    workbook.add_worksheet().set_name("Empty").unwrap();
    let big = workbook.add_worksheet().set_name("Big").unwrap();
    big.write_string(0, 0, "n").unwrap();
    for row in 1..=2100u32 {
        big.write_number(row, 0, f64::from(row)).unwrap();
    }
    workbook.save_to_buffer().unwrap()
}

#[test]
fn payload_boundary_reads_and_reopens_are_always_charged() {
    let bytes = b"headPAYgapPAY";
    let input = |limits: &Rc<RefCell<Limits>>| {
        AccountedInput::new(
            bytes,
            vec![
                EntryInflation {
                    index: 0,
                    start: 4,
                    end: 7,
                    inflated: 7,
                },
                EntryInflation {
                    index: 1,
                    start: 10,
                    end: 13,
                    inflated: 11,
                },
            ],
            Rc::clone(limits),
        )
    };
    let limits = Rc::new(RefCell::new(Limits::with_inflated_limit(17)));
    let mut reader = input(&limits);
    let mut buf = [0; 64];
    assert_eq!(reader.read(&mut buf).unwrap(), 4);
    assert_eq!(&buf[..4], b"head");
    assert_eq!(reader.read(&mut buf).unwrap(), 3);
    assert_eq!(&buf[..3], b"PAY");
    assert_eq!(reader.read(&mut buf).unwrap(), 3);
    let error = reader.read(&mut buf).unwrap_err();
    assert_eq!(
        error.to_string(),
        ConvertError::limit("inflated input", 17).to_string()
    );
    assert_eq!(
        limits.borrow().check(),
        Err(ConvertError::limit("inflated input", 17))
    );

    let limits = Rc::new(RefCell::new(Limits::with_inflated_limit(13)));
    let mut reader = input(&limits);
    reader.seek(SeekFrom::Start(4)).unwrap();
    assert_eq!(reader.read(&mut []).unwrap(), 0);
    assert_eq!(reader.read(&mut buf[..1]).unwrap(), 1);
    let mut reopened = reader.clone();
    reopened.seek(SeekFrom::Start(4)).unwrap();
    assert!(reopened.read(&mut buf[..1]).is_err());
    assert_eq!(
        limits.borrow().check(),
        Err(ConvertError::limit("inflated input", 13))
    );
}

#[test]
fn sheets_become_tables() {
    let out = convert(&workbook(), &Options::default()).unwrap();
    assert_eq!(out.format, "xlsx");
    let labels: Vec<&str> = out.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["sheet Revenue", "sheet Empty", "sheet Big"]);
    assert_eq!(
        out.sections[0].markdown,
        "## Revenue\n\n|Region|Amount|Day|\n|-|-|-|\n|EU\\|West|1200|2024-03-09|\n|US|0.3|TRUE|\n"
    );
    assert_eq!(out.sections[1].markdown, "## Empty\n\n(empty sheet)\n");
    let big = &out.sections[2].markdown;
    assert!(big.ends_with("|2000|\n\n… 100 more rows\n"), "{big}");
}

#[test]
fn repeated_worksheet_reads_share_the_inflation_ceiling() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    for uppercase in [false, true] {
        let mut workbook = Workbook::new();
        workbook.add_worksheet().write_string(0, 0, "a").unwrap();
        let original = workbook.save_to_buffer().unwrap();
        let mut source = zip::ZipArchive::new(Cursor::new(&original)).unwrap();
        let mut target = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            let mut name = entry.name().unwrap().to_string();
            let mut content = String::new();
            entry.read_to_string(&mut content).unwrap();
            if name == "xl/workbook.xml" {
                let (before, rest) = content.split_once("<sheets>").unwrap();
                let (_, after) = rest.split_once("</sheets>").unwrap();
                let sheets: String = (0..257)
                    .map(|n| format!("<sheet name='Part{n}' sheetId='{}' r:id='rId1'/>", n + 1))
                    .collect();
                content = format!("{before}<sheets>{sheets}</sheets>{after}");
            } else if name == "xl/worksheets/sheet1.xml" {
                content = content.replace(
                    "</worksheet>",
                    &format!("<!--{}--></worksheet>", "a".repeat(64 * 1024)),
                );
            }
            if uppercase {
                if name == "_rels/.rels" {
                    content = content.replace("workbook.xml", "workbook.XML");
                }
                if name == "xl/workbook.xml" {
                    name = "xl/workbook.XML".into();
                }
            }
            target
                .start_file(
                    name,
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            target.write_all(content.as_bytes()).unwrap();
        }
        let bytes = target.finish().unwrap().into_inner();
        for extension in ["XLSX", "XLSM"] {
            let path = std::path::PathBuf::from(format!("case.{extension}"));
            assert_eq!(
                crate::detect(Some(&path), &bytes),
                Some(crate::Format::Xlsx)
            );
        }
        let benign = convert(&bytes, &Options::default()).unwrap();
        assert_eq!(benign.sections.len(), 257);
        assert!(benign.sections.iter().all(|s| s.markdown.contains("|a|")));
        assert_eq!(
            convert_with_limits(&bytes, Limits::with_inflated_limit(16 * 1024 * 1024)),
            Err(ConvertError::limit("inflated input", 16 * 1024 * 1024))
        );
    }
}

#[test]
fn large_sheet_keeps_its_cells() {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let text = "a".repeat(32_760);
    for row in 0..1025 {
        sheet.write_string(row, 0, &text).unwrap();
    }
    let out = crate::convert(
        crate::Format::Xlsx,
        &workbook.save_to_buffer().unwrap(),
        &Options::default(),
    )
    .unwrap();
    let markdown = &out.sections[0].markdown;
    assert!(markdown.len() > 32 * 1024 * 1024);
    assert_eq!(markdown.matches(&text).count(), 1025);
}

#[test]
fn numbers_format_compactly() {
    assert_eq!(number(3.0), "3");
    assert_eq!(number(-2.5), "-2.5");
    assert_eq!(number(0.1 + 0.2), "0.3");
    assert_eq!(number(1234567.891), "1234567.891");
    assert_eq!(number(1e20), "100000000000000000000");
    assert_eq!(number(1.5e-7), "0.00000015");
}

#[test]
fn malformed_input_is_invalid() {
    assert!(matches!(
        convert(b"definitely not a workbook", &Options::default()),
        Err(ConvertError::Invalid(_))
    ));
    assert!(convert(b"PK\x03\x04broken", &Options::default()).is_err());
}
