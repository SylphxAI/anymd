//! CSV/TSV → a Markdown table, capped so agents do not drown in rows.

use crate::{markdown_table, ConvertError, Converted, Options, Section};

/// Data rows kept per table (the header row is extra).
pub(crate) const MAX_ROWS: usize = 2000;

pub fn convert(bytes: &[u8], delimiter: u8, _options: &Options) -> Result<Converted, ConvertError> {
    // Decode before parsing: delimiters, quotes and newlines are code points,
    // not individual bytes in a UTF-16 export.
    let bytes = crate::encoding::decode(bytes, "delimited text")?;
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(bytes.as_ref());
    let mut table = CappedTable::default();
    for record in reader.byte_records() {
        let record =
            record.map_err(|e| ConvertError::Invalid(format!("malformed delimited text: {e}")))?;
        if table.is_full() {
            table.overflow(
                record
                    .iter()
                    .any(|f| !String::from_utf8_lossy(f).trim().is_empty()),
            );
        } else {
            table.push(
                record
                    .iter()
                    .map(|f| String::from_utf8_lossy(f).into_owned())
                    .collect(),
            );
        }
    }
    Ok(Converted {
        outline: Vec::new(),
        format: if delimiter == b'\t' { "tsv" } else { "csv" }.into(),
        title: None,
        sections: vec![Section {
            label: "table".into(),
            markdown: table.finish(),
        }],
        metadata: Vec::new(),
    })
}

/// Rows destined for one Markdown table: blank rows dropped, blank edge columns
/// trimmed, and everything past the header plus [`MAX_ROWS`] counted, not kept.
#[derive(Default)]
pub(crate) struct CappedTable {
    rows: Vec<Vec<String>>,
    more: usize,
}

impl CappedTable {
    pub(crate) fn is_full(&self) -> bool {
        self.rows.len() > MAX_ROWS
    }

    pub(crate) fn push(&mut self, row: Vec<String>) {
        if row.iter().all(|c| c.trim().is_empty()) {
            return;
        }
        if self.is_full() {
            self.more += 1;
        } else {
            self.rows.push(row);
        }
    }

    /// Count a row past the cap without materialising it.
    pub(crate) fn overflow(&mut self, non_empty: bool) {
        if non_empty {
            self.more += 1;
        }
    }

    pub(crate) fn finish(mut self) -> String {
        let used = |c: usize, rows: &[Vec<String>]| {
            rows.iter()
                .any(|r| r.get(c).is_some_and(|v| !v.trim().is_empty()))
        };
        let width = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        let first = (0..width).find(|&c| used(c, &self.rows)).unwrap_or(0);
        let last = (0..width)
            .rev()
            .find(|&c| used(c, &self.rows))
            .map_or(0, |c| c + 1);
        if last <= first {
            return String::new();
        }
        for row in &mut self.rows {
            row.truncate(last);
            row.drain(..first.min(row.len()));
        }
        let mut out = markdown_table(&self.rows);
        if self.more > 0 {
            out.push_str(&format!("\n… {} more rows\n", self.more));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(input: &str, delimiter: u8) -> String {
        convert(input.as_bytes(), delimiter, &Options::default())
            .unwrap()
            .sections[0]
            .markdown
            .clone()
    }

    #[test]
    fn quoting_pipes_and_ragged_rows() {
        let input = "\u{feff}name,note,amount\n\"Smith, J\",\"says \"\"hi\"\"\",1\nA|B,\"multi\nline\",2,extra\n,,\n";
        assert_eq!(
            md(input, b','),
            "|name|note|amount||\n|-|-|-|-|\n|Smith, J|says \"hi\"|1||\n|A\\|B|multi line|2|extra|\n"
        );
    }

    #[test]
    fn tsv_and_row_cap() {
        let mut input = String::from("a\tb\n");
        for i in 0..2500 {
            input.push_str(&format!("{i}\t{}\n", i * 2));
        }
        let out = convert(input.as_bytes(), b'\t', &Options::default()).unwrap();
        assert_eq!(out.format, "tsv");
        let markdown = &out.sections[0].markdown;
        assert!(markdown.starts_with("|a|b|\n|-|-|\n|0|0|\n"));
        assert!(markdown.contains("|1999|3998|\n\n… 500 more rows\n"));
        assert!(!markdown.contains("\n|2000|"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(md("", b','), "");
        for bytes in [vec![0xEF, 0xBB, 0xBF], vec![0xFF, 0xFE], vec![0xFE, 0xFF]] {
            assert_eq!(
                convert(&bytes, b',', &Options::default()).unwrap().sections[0].markdown,
                ""
            );
        }
    }

    fn utf16(input: &str, little_endian: bool) -> Vec<u8> {
        let mut bytes = if little_endian {
            vec![0xFF, 0xFE]
        } else {
            vec![0xFE, 0xFF]
        };
        for unit in input.encode_utf16() {
            bytes.extend(if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        bytes
    }

    fn assert_encodings(input: &str, delimiter: u8, expected: &str) {
        for bytes in [
            input.as_bytes().to_vec(),
            format!("\u{feff}{input}").into_bytes(),
            utf16(input, true),
            utf16(input, false),
        ] {
            let out = convert(&bytes, delimiter, &Options::default()).unwrap();
            assert_eq!(out.sections[0].markdown, expected);
            assert!(!out.sections[0].markdown.contains(['\0', '\u{fffd}']));
        }
    }

    #[test]
    fn same_cells_across_encodings_and_delimiters() {
        for delimiter in [b',', b'\t'] {
            let d = char::from(delimiter);
            assert_encodings(
                &format!("name{d}city\r\nAlice{d}Hong Kong\r\n陳小明{d}香港\r\n"),
                delimiter,
                "|name|city|\n|-|-|\n|Alice|Hong Kong|\n|陳小明|香港|\n",
            );
            assert_encodings(
                &format!("{d}name{d}note{d}city{d}\r\n{d}\"陳小明😀{d}A\"{d}\"says \"\"hi\"\"\r\nline\"{d}香港|港{d}\r\n{d}{d}{d}{d}\r\n{d}Alice{d}short\r\n"),
                delimiter,
                &format!("|name|note|city|\n|-|-|-|\n|陳小明😀{d}A|says \"hi\" line|香港\\|港|\n|Alice|short||\n"),
            );
        }
    }

    #[test]
    fn malformed_utf16_is_an_actionable_error_and_next_conversion_works() {
        for little_endian in [true, false] {
            for delimiter in [b',', b'\t'] {
                for units in [
                    vec![0xD800],
                    vec![0xDC00],
                    vec![0xD800, 0x0041],
                    vec![0xDC00, 0xD800],
                ] {
                    let mut bytes = utf16("name\r\n", little_endian);
                    for unit in units {
                        bytes.extend(if little_endian {
                            u16::to_le_bytes(unit)
                        } else {
                            u16::to_be_bytes(unit)
                        });
                    }
                    let error = convert(&bytes, delimiter, &Options::default())
                        .unwrap_err()
                        .to_string();
                    assert!(error.contains("unpaired surrogate"), "{error}");
                    assert!(error.contains("re-export"), "{error}");
                    assert!(!md("name\nAlice\n", delimiter).is_empty());
                }
                let mut odd = utf16("name\r\nAlice\r\n", little_endian);
                odd.push(0x41);
                let error = convert(&odd, delimiter, &Options::default())
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("odd byte count"), "{error}");
                assert!(error.contains("re-export"), "{error}");
            }
        }
    }

    #[test]
    fn encoded_row_cap_counts_only_nonempty_rows_and_validates_the_tail() {
        let input = format!("a,b\r\n{}", "1,2\r\n,,\r\n".repeat(MAX_ROWS + 3));
        let expected = format!(
            "|a|b|\n|-|-|\n{}\n… 3 more rows\n",
            "|1|2|\n".repeat(MAX_ROWS)
        );
        assert_encodings(&input, b',', &expected);
        for little_endian in [true, false] {
            let mut bytes = utf16(&input, little_endian);
            bytes.extend(if little_endian {
                0xD800u16.to_le_bytes()
            } else {
                0xD800u16.to_be_bytes()
            });
            assert!(convert(&bytes, b',', &Options::default()).is_err());
        }
    }
}
