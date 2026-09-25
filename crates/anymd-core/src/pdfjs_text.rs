use lopdf::Object;

/// pdf.js `PDFStringTranslateTable` (indices with nonzero entries only).
/// Zero / missing entries keep the original byte as a Unicode code unit.
const PDF_STRING_TRANSLATE_NONEMPTY: &[(u8, u16)] = &[
    (0x18, 0x02d8),
    (0x19, 0x02c7),
    (0x1a, 0x02c6),
    (0x1b, 0x02d9),
    (0x1c, 0x02dd),
    (0x1d, 0x02db),
    (0x1e, 0x02da),
    (0x1f, 0x02dc),
    (0x80, 0x2022),
    (0x81, 0x2020),
    (0x82, 0x2021),
    (0x83, 0x2026),
    (0x84, 0x2014),
    (0x85, 0x2013),
    (0x86, 0x0192),
    (0x87, 0x2044),
    (0x88, 0x2039),
    (0x89, 0x203a),
    (0x8a, 0x2212),
    (0x8b, 0x2030),
    (0x8c, 0x201e),
    (0x8d, 0x201c),
    (0x8e, 0x201d),
    (0x8f, 0x2018),
    (0x90, 0x2019),
    (0x91, 0x201a),
    (0x92, 0x2122),
    (0x93, 0xfb01),
    (0x94, 0xfb02),
    (0x95, 0x0141),
    (0x96, 0x0152),
    (0x97, 0x0160),
    (0x98, 0x0178),
    (0x99, 0x017d),
    (0x9a, 0x0131),
    (0x9b, 0x0142),
    (0x9c, 0x0153),
    (0x9d, 0x0161),
    (0x9e, 0x017e),
    (0xa0, 0x20ac),
];

fn translate_pdf_string_byte(byte: u8) -> char {
    for &(from, to) in PDF_STRING_TRANSLATE_NONEMPTY {
        if from == byte {
            return char::from_u32(u32::from(to)).unwrap_or('\u{FFFD}');
        }
    }
    // pdf.js: `code ? String.fromCharCode(code) : str.charAt(i)` — keep original.
    char::from(byte)
}

/// Match pdf.js `stringToPDFString` for public PDF text strings:
/// - UTF-16BE BOM (`FE FF`): drop a trailing unpaired byte, then decode pairs
/// - UTF-16LE BOM (`FF FE`): drop a trailing unpaired byte, then decode pairs
/// - UTF-8 BOM (`EF BB BF`): decode UTF-8
/// - otherwise PDFStringTranslateTable with ESC-sequence stripping
///
/// Important: do **not** use lopdf `decode_text_string` / PDF_DOC_ENCODING for the
/// non-BOM path. That table maps LF/CR/HT and other control bytes to `None` and
/// `bytes_to_string` filter_maps them away, so multiline form values like
/// `(line1\nline2)` collapse to `line1line2`. pdf.js keeps those code units.
pub(crate) fn decode_pdfjs_text_string(value: &Object) -> Option<String> {
    let bytes = match value {
        Object::String(bytes, _) => bytes.as_slice(),
        // Names and non-strings are not public PDF text strings in this path.
        _ => return None,
    };
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let mut payload = &bytes[2..];
        if payload.len() % 2 == 1 {
            payload = &payload[..payload.len() - 1];
        }
        let units: Vec<u16> = payload
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();
        return String::from_utf16(&units).ok();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let mut payload = &bytes[2..];
        if payload.len() % 2 == 1 {
            payload = &payload[..payload.len() - 1];
        }
        let units: Vec<u16> = payload
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        return String::from_utf16(&units).ok();
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec()).ok();
    }
    Some(decode_pdf_string_translate_table(bytes))
}

fn decode_pdf_string_translate_table(bytes: &[u8]) -> String {
    // pdf.js default keepEscapeSequence=false: drop ESC ... ESC spans.
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == 0x1b {
            i += 1;
            while i < bytes.len() && bytes[i] != 0x1b {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // consume closing ESC when present
            }
            continue;
        }
        out.push(translate_pdf_string_byte(byte));
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::StringFormat;

    #[test]
    fn odd_length_utf16_be_drops_trailing_byte() {
        let value = Object::String(
            vec![0xFE, 0xFF, 0x00, 0x42, 0x00, 0x6F, 0x62],
            StringFormat::Hexadecimal,
        );
        assert_eq!(decode_pdfjs_text_string(&value).as_deref(), Some("Bo"));
    }

    #[test]
    fn valid_utf16_be_and_utf8_bom() {
        let utf16 = Object::String(
            vec![0xFE, 0xFF, 0x00, 0x41, 0x00, 0x64, 0x00, 0x61],
            StringFormat::Hexadecimal,
        );
        let utf8 = Object::String(
            vec![0xEF, 0xBB, 0xBF, b'H', b'i'],
            StringFormat::Hexadecimal,
        );
        assert_eq!(decode_pdfjs_text_string(&utf16).as_deref(), Some("Ada"));
        assert_eq!(decode_pdfjs_text_string(&utf8).as_deref(), Some("Hi"));
    }

    #[test]
    fn utf16_le_bom_decodes_pairs() {
        let value = Object::String(
            vec![0xFF, 0xFE, 0x41, 0x00, 0x64, 0x00, 0x61, 0x00],
            StringFormat::Hexadecimal,
        );
        assert_eq!(decode_pdfjs_text_string(&value).as_deref(), Some("Ada"));
    }

    #[test]
    fn literal_lf_and_crlf_are_preserved_like_pdfjs() {
        let lf = Object::String(b"line1\nline2".to_vec(), StringFormat::Literal);
        let crlf = Object::String(b"line1\r\nline2".to_vec(), StringFormat::Literal);
        assert_eq!(
            decode_pdfjs_text_string(&lf).as_deref(),
            Some("line1\nline2")
        );
        assert_eq!(
            decode_pdfjs_text_string(&crlf).as_deref(),
            Some("line1\r\nline2")
        );
    }

    #[test]
    fn pdfdoc_specials_translate_like_pdfjs() {
        // 0x8B is PDFDocEncoding per-mille (U+2030); 0xA0 is euro (U+20AC).
        let value = Object::String(
            vec![b't', b'e', b'x', b't', 0x8b, 0xa0],
            StringFormat::Literal,
        );
        assert_eq!(decode_pdfjs_text_string(&value).as_deref(), Some("text‰€"));
    }

    #[test]
    fn esc_sequences_are_stripped_like_pdfjs_default() {
        let value = Object::String(b"pre\x1bhidden\x1bpost".to_vec(), StringFormat::Literal);
        assert_eq!(decode_pdfjs_text_string(&value).as_deref(), Some("prepost"));
    }
}
