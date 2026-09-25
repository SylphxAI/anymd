//! CSV/TSV → a Markdown table, capped so agents do not drown in rows.

use crate::{markdown_table, ConvertError, Converted, Options, Section};

/// Data rows kept per table (the header row is extra).
pub(crate) const MAX_ROWS: usize = 2000;

pub fn convert(bytes: &[u8], delimiter: u8, _options: &Options) -> Result<Converted, ConvertError> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(bytes);
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
    fn empty_and_binary_input() {
        assert_eq!(md("", b','), "");
        assert!(convert(&[0xff, 0xfe, 0x00, b',', 0x80], b',', &Options::default()).is_ok());
    }
}
