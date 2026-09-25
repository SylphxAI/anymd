use std::fs::File;
use std::io::Read;
use std::path::Path;

use lopdf::{Document, ObjectId};
use sha2::{Digest, Sha256};

use crate::text_index::{TextIndexError, TextIndexErrorCode};

pub(crate) struct ParsedPdf {
    pub document: Document,
    pub pages: Vec<(u32, ObjectId)>,
    pub encryption_facts: Option<EncryptionFacts>,
    /// pdf.js-compatible linearization detection over the exact source bytes.
    pub is_linearized: bool,
    #[allow(dead_code)]
    pub source_hash: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EncryptionFacts {
    pub permissions: Option<i64>,
    /// Security handler name from Encrypt.Filter (pdf.js EncryptFilterName).
    /// Captured before lopdf removes the Encrypt dictionary on empty-password decrypt.
    pub filter_name: Option<String>,
}

impl ParsedPdf {
    pub fn load(path: &Path, max_file_bytes: u64) -> Result<Self, TextIndexError> {
        let file = File::open(path).map_err(|error| invalid_request(path, error))?;
        let metadata = file
            .metadata()
            .map_err(|error| invalid_request(path, error))?;
        if !metadata.is_file() {
            return Err(request_error(format!(
                "Path '{}' is not a regular file.",
                path.display()
            )));
        }
        if metadata.len() > max_file_bytes {
            return Err(request_error(format!(
                "File exceeds maximum size of {max_file_bytes} bytes."
            )));
        }
        let mut bytes =
            Vec::with_capacity(usize::try_from(metadata.len().min(max_file_bytes)).unwrap_or(0));
        file.take(max_file_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| {
                request_error(format!(
                    "Failed to read PDF bytes at '{}': {error}",
                    path.display()
                ))
            })?;
        if bytes.len() as u64 > max_file_bytes {
            return Err(request_error(format!(
                "File exceeds maximum size of {max_file_bytes} bytes."
            )));
        }
        let source_hash = format!("{:x}", Sha256::digest(&bytes));
        let is_linearized = detect_pdfjs_linearization(&bytes);
        let mut document = Document::load_mem(&bytes).map_err(extraction_error)?;
        // lopdf automatically decrypts PDFs that accept the empty password and
        // preserves the decoded security state. Capture /P from either the raw
        // Encrypt dictionary or that preserved state before any explicit decrypt.
        let encryption_facts = read_encryption_facts(&document);
        if document.is_encrypted() {
            document.decrypt("").map_err(|error| {
                extraction_error(format!(
                    "encrypted PDF requires a supported password: {error}"
                ))
            })?;
        }
        let pages = document.get_pages().into_iter().collect();
        Ok(Self {
            document,
            pages,
            encryption_facts,
            is_linearized,
            source_hash,
        })
    }
}

fn read_encryption_facts(document: &Document) -> Option<EncryptionFacts> {
    if let Ok(dictionary) = document.get_encrypted() {
        let filter_name = dictionary
            .get(b"Filter")
            .ok()
            .and_then(|value| value.as_name().ok())
            .and_then(|value| std::str::from_utf8(value).ok())
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty());
        return Some(EncryptionFacts {
            permissions: dictionary
                .get(b"P")
                .ok()
                .and_then(|value| value.as_i64().ok()),
            filter_name,
        });
    }
    // lopdf only supports the Standard security handler and removes Encrypt on
    // empty-password decrypt. When encryption_state remains, pdf.js still exposes
    // EncryptFilterName="Standard".
    document
        .encryption_state
        .as_ref()
        .map(|state| EncryptionFacts {
            permissions: Some(state.permissions().bits() as i64),
            filter_name: Some("Standard".to_string()),
        })
}

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

#[derive(Debug)]
enum PdfToken {
    Int(i64),
    Real(f64),
    Name(Vec<u8>),
}

#[derive(Debug)]
#[allow(dead_code)]
enum PdfValue {
    Int(i64),
    Real(f64),
    Name(Vec<u8>),
    Array(Vec<PdfValue>),
    Dict(std::collections::BTreeMap<Vec<u8>, PdfValue>),
}

struct PdfLexer<'a> {
    bytes: &'a [u8],
    idx: usize,
}

impl<'a> PdfLexer<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, idx: 0 }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while self.idx < self.bytes.len()
                && matches!(self.bytes[self.idx], b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
            {
                self.idx += 1;
            }
            if self.idx < self.bytes.len() && self.bytes[self.idx] == b'%' {
                while self.idx < self.bytes.len()
                    && self.bytes[self.idx] != b'\n'
                    && self.bytes[self.idx] != b'\r'
                {
                    self.idx += 1;
                }
                continue;
            }
            break;
        }
    }

    fn next_token(&mut self) -> Option<PdfToken> {
        self.skip_ws_and_comments();
        if self.idx >= self.bytes.len() {
            return None;
        }
        let b = self.bytes[self.idx];
        if b == b'/' {
            self.idx += 1;
            let start = self.idx;
            while self.idx < self.bytes.len() {
                let c = self.bytes[self.idx];
                if matches!(
                    c,
                    b' ' | b'\t'
                        | b'\n'
                        | b'\r'
                        | 0x0c
                        | b'%'
                        | b'('
                        | b')'
                        | b'<'
                        | b'>'
                        | b'['
                        | b']'
                        | b'{'
                        | b'}'
                        | b'/'
                ) {
                    break;
                }
                self.idx += 1;
            }
            return Some(PdfToken::Name(self.bytes[start..self.idx].to_vec()));
        }
        if b.is_ascii_digit() || b == b'+' || b == b'-' || b == b'.' {
            let start = self.idx;
            self.idx += 1;
            while self.idx < self.bytes.len() {
                let c = self.bytes[self.idx];
                if c.is_ascii_digit() || c == b'.' {
                    self.idx += 1;
                } else {
                    break;
                }
            }
            let raw = std::str::from_utf8(&self.bytes[start..self.idx]).ok()?;
            if raw.contains('.') {
                return Some(PdfToken::Real(raw.parse().ok()?));
            }
            return Some(PdfToken::Int(raw.parse().ok()?));
        }
        // bare word (obj, endobj, true, false, null)
        if b.is_ascii_alphabetic() {
            let start = self.idx;
            self.idx += 1;
            while self.idx < self.bytes.len() {
                let c = self.bytes[self.idx];
                if c.is_ascii_alphanumeric() {
                    self.idx += 1;
                } else {
                    break;
                }
            }
            return Some(PdfToken::Name(self.bytes[start..self.idx].to_vec()));
        }
        // single-char tokens not needed beyond dict/array markers handled in readers
        None
    }

    fn read_dict(&mut self) -> Option<std::collections::BTreeMap<Vec<u8>, PdfValue>> {
        self.skip_ws_and_comments();
        if self.idx + 1 >= self.bytes.len() || &self.bytes[self.idx..self.idx + 2] != b"<<" {
            return None;
        }
        self.idx += 2;
        let mut dict = std::collections::BTreeMap::new();
        loop {
            self.skip_ws_and_comments();
            if self.idx + 1 < self.bytes.len() && &self.bytes[self.idx..self.idx + 2] == b">>" {
                self.idx += 2;
                break;
            }
            let Some(PdfToken::Name(key)) = self.next_token() else {
                return None;
            };
            let value = self.read_value()?;
            dict.insert(key, value);
        }
        Some(dict)
    }

    fn read_value(&mut self) -> Option<PdfValue> {
        self.skip_ws_and_comments();
        if self.idx + 1 < self.bytes.len() && &self.bytes[self.idx..self.idx + 2] == b"<<" {
            return self.read_dict().map(PdfValue::Dict);
        }
        if self.idx < self.bytes.len() && self.bytes[self.idx] == b'[' {
            self.idx += 1;
            let mut values = Vec::new();
            loop {
                self.skip_ws_and_comments();
                if self.idx < self.bytes.len() && self.bytes[self.idx] == b']' {
                    self.idx += 1;
                    break;
                }
                values.push(self.read_value()?);
            }
            return Some(PdfValue::Array(values));
        }
        match self.next_token()? {
            PdfToken::Int(v) => Some(PdfValue::Int(v)),
            PdfToken::Real(v) => Some(PdfValue::Real(v)),
            PdfToken::Name(v) => Some(PdfValue::Name(v)),
        }
    }
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

fn invalid_request(path: &Path, error: std::io::Error) -> TextIndexError {
    request_error(format!(
        "Unable to access file at '{}': {error}",
        path.display()
    ))
}

fn request_error(message: String) -> TextIndexError {
    TextIndexError {
        code: TextIndexErrorCode::InvalidRequest,
        message,
    }
}

fn extraction_error(error: impl std::fmt::Display) -> TextIndexError {
    TextIndexError {
        code: TextIndexErrorCode::ExtractionFailed,
        message: format!("Failed to extract PDF: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::detect_pdfjs_linearization;

    fn wrap_first_object(dict: &str) -> Vec<u8> {
        let make = |l_value: &str| -> Vec<u8> {
            let dict_body = dict.replace("/L 0", &format!("/L {l_value}"));
            let mut parts: Vec<u8> = b"%PDF-1.4\n".to_vec();
            parts.extend_from_slice(&[b'%', 0xe2, 0xe3, 0xcf, 0xd3, b'\n']);
            let obj1 = format!("1 0 obj\n{dict_body}\nendobj\n");
            let obj2 = "2 0 obj\n<< /Type /Catalog /Pages 3 0 R >>\nendobj\n";
            let obj3 = "3 0 obj\n<< /Type /Pages /Kids [4 0 R] /Count 1 >>\nendobj\n";
            let obj4 = "4 0 obj\n<< /Type /Page /Parent 3 0 R /MediaBox [0 0 612 792] /Contents 5 0 R >>\nendobj\n";
            let obj5 = "5 0 obj\n<< /Length 20 >>\nstream\nBT /F1 1 Tf ET\nendstream\nendobj\n";
            let mut offsets = [0usize; 6];
            offsets[1] = parts.len();
            parts.extend_from_slice(obj1.as_bytes());
            offsets[2] = parts.len();
            parts.extend_from_slice(obj2.as_bytes());
            offsets[3] = parts.len();
            parts.extend_from_slice(obj3.as_bytes());
            offsets[4] = parts.len();
            parts.extend_from_slice(obj4.as_bytes());
            offsets[5] = parts.len();
            parts.extend_from_slice(obj5.as_bytes());
            let xref_pos = parts.len();
            let mut xref = String::from("xref\n0 6\n0000000000 65535 f \n");
            for i in 1..=5 {
                xref.push_str(&format!("{:010} 00000 n \n", offsets[i]));
            }
            parts.extend_from_slice(xref.as_bytes());
            parts.extend_from_slice(
                format!("trailer\n<< /Size 6 /Root 2 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n")
                    .as_bytes(),
            );
            parts
        };

        if dict.contains("/L 0") {
            let mut bytes = make("1");
            for _ in 0..8 {
                let desired = bytes.len().to_string();
                bytes = make(&desired);
                if bytes.len().to_string() == desired {
                    return bytes;
                }
            }
            return bytes;
        }
        make("0")
    }

    #[test]
    fn valid_linearization_dict_is_detected() {
        let bytes =
            wrap_first_object("<< /Linearized 1 /L 0 /H [100 200] /O 4 /E 50 /N 1 /T 900 >>");
        assert!(
            detect_pdfjs_linearization(&bytes),
            "bytes len {} head {}",
            bytes.len(),
            String::from_utf8_lossy(&bytes[..bytes.len().min(160)])
        );
    }

    #[test]
    fn mismatched_l_is_not_linearized() {
        let bytes =
            wrap_first_object("<< /Linearized 1 /L 999999 /H [100 200] /O 4 /E 50 /N 1 /T 900 >>");
        assert!(!detect_pdfjs_linearization(&bytes));
    }

    #[test]
    fn zero_hint_is_not_linearized() {
        let bytes = wrap_first_object("<< /Linearized 1 /L 0 /H [0 0] /O 4 /E 50 /N 1 /T 900 >>");
        assert!(!detect_pdfjs_linearization(&bytes));
    }

    #[test]
    fn plain_first_object_is_not_linearized() {
        let bytes = wrap_first_object("<< /Type /Catalog /Pages 3 0 R >>");
        assert!(!detect_pdfjs_linearization(&bytes));
    }
}
