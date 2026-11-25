use super::PdfValue;

mod lexer;

#[cfg(test)]
mod tests;

use lexer::{PdfLexer, PdfToken};

/// Match pdf.js `Linearization.create` / `IsLinearized`:
/// first object after the header must be a linearization dictionary with
/// Linearized>0, L==stream length, valid H hints, and O/E/N/T integers.
pub(crate) fn detect_pdfjs_linearization(bytes: &[u8]) -> bool {
    let Some(rest) = skip_pdf_header(bytes) else {
        return false;
    };
    let mut lexer = PdfLexer::new(rest);
    let Some(PdfToken::Int(_)) = lexer.next_token() else {
        return false;
    };
    let Some(PdfToken::Int(_)) = lexer.next_token() else {
        return false;
    };
    let Some(PdfToken::Name(cmd)) = lexer.next_token() else {
        return false;
    };
    // Lexer returns command-like tokens as Name for bare words; require "obj".
    if cmd != b"obj" {
        // Also accept Cmd-style: our lexer yields Word for bare keywords.
        return false;
    }
    let Some(dict) = lexer.read_dict() else {
        return false;
    };
    let Some(linearized) = dict.get(b"Linearized".as_slice()).and_then(number_value) else {
        return false;
    };
    if !(linearized > 0.0 && linearized.is_finite()) {
        return false;
    }
    let Some(length) = dict.get(b"L".as_slice()).and_then(positive_int) else {
        return false;
    };
    if length != bytes.len() {
        return false;
    }
    let Some(hints) = dict.get(b"H".as_slice()).and_then(array_value) else {
        return false;
    };
    if !(hints.len() == 2 || hints.len() == 4) {
        return false;
    }
    if !hints.iter().all(|value| positive_int(value).is_some()) {
        return false;
    }
    for key in [
        b"O".as_slice(),
        b"E".as_slice(),
        b"N".as_slice(),
        b"T".as_slice(),
    ] {
        if dict.get(key).and_then(positive_int).is_none() {
            return false;
        }
    }
    if let Some(page_first) = dict.get(b"P".as_slice()) {
        if nonnegative_int(page_first).is_none() {
            return false;
        }
    }
    true
}

fn skip_pdf_header(bytes: &[u8]) -> Option<&[u8]> {
    // Robust byte scan: find %PDF-
    let start = bytes.windows(5).position(|window| window == b"%PDF-")?;
    let mut idx = start + 5;
    // skip version line
    while idx < bytes.len() && bytes[idx] != b'\n' && bytes[idx] != b'\r' {
        idx += 1;
    }
    while idx < bytes.len() && (bytes[idx] == b'\n' || bytes[idx] == b'\r') {
        idx += 1;
    }
    // optional binary comment line starting with %
    if idx < bytes.len() && bytes[idx] == b'%' {
        while idx < bytes.len() && bytes[idx] != b'\n' && bytes[idx] != b'\r' {
            idx += 1;
        }
        while idx < bytes.len() && (bytes[idx] == b'\n' || bytes[idx] == b'\r') {
            idx += 1;
        }
    }
    // skip whitespace
    while idx < bytes.len() && matches!(bytes[idx], b' ' | b'\t' | b'\n' | b'\r' | 0x0c) {
        idx += 1;
    }
    Some(&bytes[idx..])
}

fn number_value(value: &PdfValue) -> Option<f64> {
    match value {
        PdfValue::Int(v) => Some(*v as f64),
        PdfValue::Real(v) => Some(*v),
        _ => None,
    }
}

fn positive_int(value: &PdfValue) -> Option<usize> {
    match value {
        PdfValue::Int(v) if *v > 0 => usize::try_from(*v).ok(),
        PdfValue::Real(v) if *v > 0.0 && v.fract() == 0.0 => {
            if *v > 0.0 && *v <= usize::MAX as f64 {
                Some(*v as usize)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn nonnegative_int(value: &PdfValue) -> Option<usize> {
    match value {
        PdfValue::Int(v) if *v >= 0 => usize::try_from(*v).ok(),
        PdfValue::Real(v) if *v >= 0.0 && v.fract() == 0.0 => {
            if *v <= usize::MAX as f64 {
                Some(*v as usize)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn array_value(value: &PdfValue) -> Option<&[PdfValue]> {
    match value {
        PdfValue::Array(values) => Some(values.as_slice()),
        _ => None,
    }
}
