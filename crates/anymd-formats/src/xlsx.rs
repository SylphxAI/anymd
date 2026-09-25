//! XLSX/XLS/XLSB/ODS → Markdown: one table per sheet via calamine.

use std::io::Cursor;

use calamine::{Data, Reader};

use crate::csv::CappedTable;
use crate::ooxml::{self, Package};
use crate::{ConvertError, Converted, Options, Section};

pub fn convert(bytes: &[u8], _options: &Options) -> Result<Converted, ConvertError> {
    let (title, metadata) = if bytes.starts_with(b"PK\x03\x04") {
        ooxml::guard_zip(bytes)?;
        match Package::open(bytes, "spreadsheet") {
            Ok(mut package) => ooxml::core_properties(&mut package),
            Err(_) => (None, Vec::new()),
        }
    } else {
        (None, Vec::new())
    };
    // calamine is third-party parsing of hostile input: contain any panic.
    let sections = std::panic::catch_unwind(|| sheets(bytes))
        .map_err(|_| ooxml::invalid("spreadsheet parser failed on malformed input"))??;
    Ok(Converted {
        format: "xlsx".into(),
        title,
        sections,
        metadata,
    })
}

fn sheets(bytes: &[u8]) -> Result<Vec<Section>, ConvertError> {
    let mut workbook = calamine::open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| ooxml::invalid(format!("not a readable spreadsheet: {e}")))?;
    let mut sections = Vec::new();
    for name in workbook.sheet_names() {
        let markdown = match workbook.worksheet_range(&name) {
            Ok(range) => {
                let mut table = CappedTable::default();
                for row in range.rows() {
                    if table.is_full() {
                        table.overflow(
                            row.iter()
                                .any(|c| !matches!(c, Data::Empty) && !cell(c).trim().is_empty()),
                        );
                    } else {
                        table.push(row.iter().map(cell).collect());
                    }
                }
                table.finish()
            }
            // Chart sheets and dialog sheets have no cell grid.
            Err(_) => String::new(),
        };
        let markdown = if markdown.is_empty() {
            "(empty sheet)\n".to_string()
        } else {
            markdown
        };
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
    fn sheets_become_tables() {
        let out = convert(&workbook(), &Options::default()).unwrap();
        assert_eq!(out.format, "xlsx");
        let labels: Vec<&str> = out.sections.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["sheet Revenue", "sheet Empty", "sheet Big"]);
        assert_eq!(
            out.sections[0].markdown,
            "|Region|Amount|Day|\n|-|-|-|\n|EU\\|West|1200|2024-03-09|\n|US|0.3|TRUE|\n"
        );
        assert_eq!(out.sections[1].markdown, "(empty sheet)\n");
        let big = &out.sections[2].markdown;
        assert!(big.ends_with("|2000|\n\n… 100 more rows\n"), "{big}");
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
