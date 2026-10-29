//! BOM decoding shared by the text, HTML and delimited-text readers.

use std::borrow::Cow;

use crate::ConvertError;

pub(crate) fn has_bom(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
        || bytes.starts_with(&[0xEF, 0xBB, 0xBF])
}

/// Borrow UTF-8 input as before; strictly decode BOM-marked UTF-16. Validate
/// the whole input, including rows past the display cap, without a second
/// allocation for UTF-16 code units or silently replacing invalid surrogates.
pub(crate) fn decode<'a>(bytes: &'a [u8], kind: &str) -> Result<Cow<'a, [u8]>, ConvertError> {
    let (little_endian, encoding) = if bytes.starts_with(&[0xFF, 0xFE]) {
        (true, "UTF-16LE")
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        (false, "UTF-16BE")
    } else {
        return Ok(Cow::Borrowed(
            bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes),
        ));
    };
    let invalid = |reason| {
        ConvertError::Invalid(format!(
            "malformed {encoding} {kind}: {reason}; re-export as valid {encoding} with a BOM or UTF-8"
        ))
    };
    let payload = &bytes[2..];
    if payload.len() % 2 != 0 {
        return Err(invalid("incomplete two-byte code unit (odd byte count)"));
    }
    let units = payload.chunks_exact(2).map(|pair| {
        let pair = [pair[0], pair[1]];
        if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    });
    let mut text = String::with_capacity(payload.len());
    for character in char::decode_utf16(units) {
        text.push(character.map_err(|_| invalid("unpaired surrogate"))?);
    }
    Ok(Cow::Owned(text.into_bytes()))
}

/// HTML and plain text retain the UTF-8 reader's behavior, but UTF-16 must
/// be well-formed and textual. A BOM is not permission to render binary data.
pub(crate) fn text<'a>(bytes: &'a [u8], kind: &str) -> Result<Cow<'a, str>, ConvertError> {
    let decoded = decode(bytes, kind)?;
    if (bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]))
        && decoded.iter().any(|&b| binary_control(char::from(b)))
    {
        return Err(ConvertError::Invalid(format!(
                "invalid UTF-16 {kind}: binary control characters; re-export as text in UTF-8 or valid UTF-16 with a BOM"
            )));
    }
    Ok(match decoded {
        Cow::Borrowed(bytes) => String::from_utf8_lossy(bytes),
        // Only the strict UTF-16 decoder produces owned bytes, always UTF-8.
        Cow::Owned(bytes) => Cow::Owned(String::from_utf8(bytes).expect("decoded UTF-16 is UTF-8")),
    })
}

pub(crate) fn binary_control(ch: char) -> bool {
    ch <= '\u{1f}' && !matches!(ch, '\t' | '\r' | '\n' | '\u{c}')
}

/// A bounded detection hint, not document decoding. The head may end halfway
/// through a surrogate pair or UTF-8 character. Conversion validates the full
/// input; replacement here only lets a malformed BOM document reach that error.
pub(crate) fn prefix(bytes: &[u8]) -> Cow<'_, str> {
    let bytes = &bytes[..bytes.len().min(512)];
    let little = bytes.starts_with(&[0xFF, 0xFE]);
    if little || bytes.starts_with(&[0xFE, 0xFF]) {
        let units = bytes[2..].chunks_exact(2).map(|pair| {
            let pair = [pair[0], pair[1]];
            if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        return Cow::Owned(
            char::decode_utf16(units)
                .map(|ch| ch.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect(),
        );
    }
    String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes))
}
