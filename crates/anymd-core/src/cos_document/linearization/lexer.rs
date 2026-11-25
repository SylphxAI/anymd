use super::PdfValue;

#[derive(Debug)]
pub(super) enum PdfToken {
    Int(i64),
    Real(f64),
    Name(Vec<u8>),
}

pub(super) struct PdfLexer<'a> {
    bytes: &'a [u8],
    idx: usize,
}

impl<'a> PdfLexer<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
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

    pub(super) fn next_token(&mut self) -> Option<PdfToken> {
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

    pub(super) fn read_dict(&mut self) -> Option<std::collections::BTreeMap<Vec<u8>, PdfValue>> {
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
