//! XLSX/XLS/XLSB/ODS → Markdown: one table per sheet via calamine.

use crate::limits::Limits;
use std::cell::RefCell;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::rc::Rc;

use calamine::{Data, Reader};

use crate::csv::CappedTable;
use crate::ooxml::{self, EntryInflation, Package};
use crate::{ConvertError, Converted, Options, Section};

pub fn convert(bytes: &[u8], _options: &Options) -> Result<Converted, ConvertError> {
    convert_with_limits(bytes, Limits::default())
}

fn convert_with_limits(bytes: &[u8], mut limits: Limits) -> Result<Converted, ConvertError> {
    let mut entries = Vec::new();
    let (title, metadata) = if zip::ZipArchive::new(Cursor::new(bytes)).is_ok() {
        ooxml::guard_zip(bytes)?;
        let mut package = Package::open(bytes, "spreadsheet")?;
        package.limits = limits;
        entries = package.preflight()?;
        let properties = ooxml::core_properties(&mut package);
        package.limits.check()?;
        limits = package.limits;
        properties
    } else {
        (None, Vec::new())
    };
    let limits = Rc::new(RefCell::new(limits));
    let input = AccountedInput {
        cursor: Cursor::new(bytes),
        entries: Rc::new(entries),
        limits: Rc::clone(&limits),
        active: None,
    };
    // calamine is third-party parsing of hostile input: contain any panic.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sheets(input, &limits)));
    // Optional parser reads and format probing may swallow I/O errors. A limit
    // remains sticky across every cloned input and wins over parser diagnostics.
    limits.borrow().check()?;
    let sections =
        result.map_err(|_| ooxml::invalid("spreadsheet parser failed on malformed input"))??;
    Ok(Converted {
        outline: Vec::new(),
        format: "xlsx".into(),
        title,
        sections,
        metadata,
    })
}

/// Reserve the measured inflation of an entry before the parser starts reading
/// its compressed payload. Each reopen reserves it again. This covers all ZIP
/// formats and initialization attempts without duplicating calamine's parsers.
/// A partial read still reserves the whole entry, never just its compressed size.
#[derive(Clone)]
struct AccountedInput<'a> {
    cursor: Cursor<&'a [u8]>,
    entries: Rc<Vec<EntryInflation>>,
    limits: Rc<RefCell<Limits>>,
    active: Option<usize>,
}

impl Read for AccountedInput<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut limits = self.limits.borrow_mut();
        limits.check().map_err(std::io::Error::other)?;
        if buf.is_empty() {
            return Ok(0);
        }
        let position = self.cursor.position();
        let next = self
            .entries
            .partition_point(|entry| entry.start <= position);
        let entry = next
            .checked_sub(1)
            .map(|index| &self.entries[index])
            .filter(|entry| position < entry.end);
        let boundary = if let Some(entry) = entry {
            if self.active != Some(entry.index) {
                limits
                    .inflated(entry.inflated)
                    .map_err(std::io::Error::other)?;
                self.active = Some(entry.index);
            }
            Some(entry.end)
        } else {
            self.active = None;
            self.entries.get(next).map(|entry| entry.start)
        };
        // Do not let a read that starts in a header cross into an uncharged
        // payload, or let one entry's read consume the following entry.
        let count = boundary.map_or(buf.len(), |end| {
            buf.len()
                .min(usize::try_from(end - position).unwrap_or(usize::MAX))
        });
        self.cursor.read(&mut buf[..count])
    }
}

impl Seek for AccountedInput<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let after = self.cursor.seek(position)?;
        self.active = None;
        Ok(after)
    }
}

fn sheets(
    input: AccountedInput<'_>,
    limits: &Rc<RefCell<Limits>>,
) -> Result<Vec<Section>, ConvertError> {
    let mut workbook = calamine::open_workbook_auto_from_rs(input)
        .map_err(|e| ooxml::invalid(format!("not a readable spreadsheet: {e}")))?;
    limits.borrow().check()?;
    let mut sections = Vec::new();
    for name in workbook.sheet_names() {
        let range = workbook.worksheet_range(&name);
        limits.borrow().check()?;
        let markdown = match range {
            Ok(range) => {
                let mut table = CappedTable::default();
                for row in range.rows() {
                    if table.is_full() {
                        table.overflow(
                            row.iter()
                                .any(|c| !matches!(c, Data::Empty) && !cell(c).trim().is_empty()),
                        );
                    } else {
                        table.push(row.iter().map(cell).collect())?;
                    }
                }
                table.finish()?
            }
            // Chart sheets and dialog sheets have no cell grid.
            Err(_) => String::new(),
        };
        let markdown = if markdown.is_empty() {
            "(empty sheet)\n".to_string()
        } else {
            markdown
        };
        // A named sheet's name heads its table: it is the table's title on
        // the sheet tab, and often the only one. "Sheet1" says nothing.
        let default_name = name
            .to_ascii_lowercase()
            .strip_prefix("sheet")
            .is_some_and(|rest| rest.trim().chars().all(|c| c.is_ascii_digit()));
        let markdown = if default_name {
            markdown
        } else {
            format!("## {name}\n\n{markdown}")
        };
        limits.borrow_mut().output(&markdown)?;
        sections.push(Section {
            label: format!("sheet {name}"),
            markdown,
        });
    }
    Ok(sections)
}

fn cell(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => number(*f),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Data::Error(e) => e.to_string(),
        Data::DateTime(dt) => {
            if dt.is_duration() {
                let total = (dt.as_f64() * 86_400.0).round() as i64;
                let sign = if total < 0 { "-" } else { "" };
                let total = total.abs();
                return format!(
                    "{sign}{}:{:02}:{:02}",
                    total / 3600,
                    total / 60 % 60,
                    total % 60
                );
            }
            let (y, mo, d, h, mi, s, ms) = dt.to_ymd_hms_milli();
            let serial = dt.as_f64();
            if (0.0..1.0).contains(&serial) {
                format!("{h:02}:{mi:02}:{s:02}")
            } else if (h, mi, s, ms) == (0, 0, 0, 0) {
                format!("{y:04}-{mo:02}-{d:02}")
            } else {
                format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
            }
        }
    }
}

/// Numbers as Excel shows them by default: integers without `.0`, at most 15 significant digits.
fn number(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let magnitude = value.abs();
    if !(1e-5..1e15).contains(&magnitude) {
        return format!("{value}");
    }
    let digits = magnitude.log10().floor() as i32 + 1;
    let decimals = (15 - digits).clamp(0, 15) as usize;
    let formatted = format!("{value:.decimals$}");
    let trimmed = if formatted.contains('.') {
        formatted.trim_end_matches('0').trim_end_matches('.')
    } else {
        &formatted
    };
    if trimmed == "-0" {
        "0".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

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
        let limits = Rc::new(RefCell::new(Limits::with_inflated_limit(17)));
        let mut input = AccountedInput {
            cursor: Cursor::new(bytes.as_slice()),
            entries: Rc::new(vec![
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
            ]),
            limits: Rc::clone(&limits),
            active: None,
        };
        let mut buf = [0; 64];
        assert_eq!(input.read(&mut buf).unwrap(), 4);
        assert_eq!(&buf[..4], b"head");
        assert_eq!(input.read(&mut buf).unwrap(), 3);
        assert_eq!(&buf[..3], b"PAY");
        assert_eq!(input.read(&mut buf).unwrap(), 3);
        let error = input.read(&mut buf).unwrap_err();
        assert_eq!(
            error.to_string(),
            ConvertError::limit("inflated input", 17).to_string()
        );
        assert_eq!(
            limits.borrow().check(),
            Err(ConvertError::limit("inflated input", 17))
        );

        let limits = Rc::new(RefCell::new(Limits::with_inflated_limit(13)));
        input.limits = Rc::clone(&limits);
        input.seek(SeekFrom::Start(4)).unwrap();
        assert_eq!(input.read(&mut []).unwrap(), 0);
        assert_eq!(input.read(&mut buf[..1]).unwrap(), 1);
        let mut reopened = input.clone();
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
        use std::io::{Read, Write};
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
}
