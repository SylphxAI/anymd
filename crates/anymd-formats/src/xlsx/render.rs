use calamine::{Data, Range};

use crate::csv::CappedTable;
use crate::ConvertError;

pub(super) fn sheet(name: &str, range: Option<Range<Data>>) -> Result<String, ConvertError> {
    let markdown = match range {
        Some(range) => {
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
        None => String::new(),
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
    Ok(if default_name {
        markdown
    } else {
        format!("## {name}\n\n{markdown}")
    })
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
pub(super) fn number(value: f64) -> String {
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
