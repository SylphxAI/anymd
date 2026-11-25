use std::time::Duration;

use crate::tool;

use super::{decimal::restore_decimal_commas, OcrWord};

const OCR_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn ocr_section(bytes: &[u8], format: &str) -> String {
    let Some(tesseract) = tool::find("tesseract") else {
        return "_OCR was requested but needs `tesseract` installed (e.g. `apt install tesseract-ocr` or `brew install tesseract`)._".into();
    };
    let suffix = format!(".{}", format.to_ascii_lowercase());
    let result = tool::temp_file(bytes, &suffix).and_then(|file| {
        let path = file.path().as_os_str().to_owned();
        let output = tool::run(
            &tesseract,
            [path.as_os_str(), "stdout".as_ref()],
            OCR_TIMEOUT,
        )?;
        if output.success {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(format!(
                "tesseract failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    });
    match result {
        Ok(text) => {
            let text = tidy_ocr(&text);
            if text.is_empty() {
                "## Text (OCR)\n\n_No text found._".into()
            } else {
                format!("## Text (OCR)\n\n{text}")
            }
        }
        Err(error) => format!("_OCR failed: {error}_"),
    }
}

/// True when a local `tesseract` binary is on PATH.
pub fn ocr_available() -> bool {
    tool::find("tesseract").is_some()
}

/// OCR an image (PNG/JPEG/...) with the local `tesseract`; returns tidy text.
pub fn ocr_text(bytes: &[u8], suffix: &str) -> Result<String, String> {
    let tesseract = tool::find("tesseract")
        .ok_or_else(|| "OCR needs `tesseract` installed (e.g. `apt install tesseract-ocr` or `brew install tesseract`).".to_string())?;
    let file = tool::temp_file(bytes, suffix)?;
    let path = file.path().as_os_str().to_owned();
    let output = tool::run(
        &tesseract,
        [path.as_os_str(), "stdout".as_ref()],
        OCR_TIMEOUT,
    )?;
    if output.success {
        Ok(tidy_ocr(&String::from_utf8_lossy(&output.stdout)))
    } else {
        Err(format!(
            "tesseract failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// OCR an image with the local `tesseract` and return every word with its
/// box, so the caller can lay the page out (paragraphs, columns, tables)
/// instead of taking tesseract's plain text. One tesseract thread: callers
/// run several pages at once.
pub fn ocr_words(bytes: &[u8], suffix: &str) -> Result<Vec<OcrWord>, String> {
    let tesseract = tool::find("tesseract")
        .ok_or_else(|| "OCR needs `tesseract` installed (e.g. `apt install tesseract-ocr` or `brew install tesseract`).".to_string())?;
    let file = tool::temp_file(bytes, suffix)?;
    let path = file.path().as_os_str().to_owned();
    let output = tool::run_with_env(
        &tesseract,
        [path.as_os_str(), "stdout".as_ref(), "tsv".as_ref()],
        &[("OMP_THREAD_LIMIT", "1")],
        OCR_TIMEOUT,
    )?;
    if !output.success {
        return Err(format!(
            "tesseract failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut words = parse_tsv(&String::from_utf8_lossy(&output.stdout));
    restore_decimal_commas(&mut words, bytes);
    Ok(words)
}

/// Words (level 5 rows) from tesseract's TSV output.
pub fn parse_tsv(tsv: &str) -> Vec<OcrWord> {
    let mut out = Vec::new();
    for line in tsv.lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 12 || fields[0] != "5" {
            continue;
        }
        let number = |i: usize| fields[i].trim().parse::<u32>().ok();
        let (Some(block), Some(par), Some(row), Some(left), Some(top), Some(width), Some(height)) = (
            number(2),
            number(3),
            number(4),
            number(6),
            number(7),
            number(8),
            number(9),
        ) else {
            continue;
        };
        let confidence = fields[10].trim().parse::<f32>().unwrap_or(-1.0);
        let text = fields[11..].join("\t").trim().to_string();
        if text.is_empty() || confidence < 0.0 {
            continue;
        }
        out.push(OcrWord {
            left,
            top,
            width,
            height,
            line: (block, par, row),
            confidence,
            text,
        });
    }
    out
}

fn tidy_ocr(text: &str) -> String {
    let mut out = Vec::new();
    let mut blank = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push(String::new());
            blank = false;
        }
        out.push(line.to_string());
    }
    out.join("\n")
}
