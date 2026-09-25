//! Page content stream checks that run before text extraction, including inline image length checks.

use super::*;

/// Pre-validate every page content stream before `output_doc` parses it.
///
/// lopdf's content parser panics (rather than returning
/// `Parse(InvalidContentStream)`) on inline images whose dict cannot yield an
/// image length — notably a missing `/CS` entry when `/IM` is not true, which
/// `unwrap()`s a `DictKey("ColorSpace")` error. A panic inside
/// `Content::decode` would escape as a worker abort; instead each page is
/// decoded defensively and inline-image dicts are length-checked up front so
/// the failure below becomes `Failed to extract PDF text: invalid content
/// stream (page N)`.
pub(super) fn validate_page_content_streams(doc: &Document) -> Result<(), TextIndexError> {
    for (page_number, object_id) in doc.get_pages() {
        let content = doc.get_page_content(object_id).map_err(|err| {
            TextIndexError::extraction_failed(format!(
                "Failed to extract PDF text: invalid content stream (page {page_number}): {err}"
            ))
        })?;
        validate_inline_images(&content).map_err(|detail| {
            TextIndexError::extraction_failed(format!(
                "Failed to extract PDF text: invalid content stream (page {page_number}): {detail}"
            ))
        })?;
        // `Content::decode` itself panics on the shapes above (the `cut`
        // combinator turns the `unwrap()` into a `Failure`, which the
        // non-strict entry point still propagates by panic). Run it under
        // `catch_unwind` so any remaining malformed stream — inline image or
        // otherwise — is a page-level tool error, never a worker abort.
        // `Content::decode` takes `&[u8]`; wrap the call so the closure is
        // `UnwindSafe` without relying on `AssertUnwindSafe`.
        let decode_result = std::panic::catch_unwind(|| {
            lopdf::content::Content::decode(&content).map(|_| ())
        });
        match decode_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                return Err(TextIndexError::extraction_failed(format!(
                    "Failed to extract PDF text: invalid content stream (page {page_number}): {err}"
                )));
            }
            Err(_) => {
                return Err(TextIndexError::extraction_failed(format!(
                    "Failed to extract PDF text: invalid content stream (page {page_number}): content stream parse failure"
                )));
            }
        }
    }
    Ok(())
}

/// Validate `BI ... ID <data> EI` constructs in a raw content stream.
///
/// For each inline image, the expected data length is computed from the
/// inline dict (`/W`+`/H`+`/BPC`, plus `/CS` component count unless
/// `/IM true`), and `EI` must follow exactly after that many data bytes
/// (modulo the single whitespace lopdf requires on each side). Shapes that
/// cannot yield a length — missing `/CS` without `/IM true`, unknown
/// colorspaces, `/Filter` entries, zero dimensions — are rejected here
/// instead of reaching lopdf's `unwrap()`. Unknown operators or non-inline
/// content passes through untouched; only a structurally-invalid inline
/// image fails validation.
pub(super) fn validate_inline_images(content: &[u8]) -> Result<(), String> {
    let mut cursor = content;
    while let Some(bi_offset) = find_inline_begin(cursor) {
        cursor = &cursor[bi_offset + 2..];
        let after_bi = skip_content_whitespace(cursor);
        // Parse `key value` pairs until the `ID` operator.
        let mut dict: Vec<(&[u8], &[u8])> = Vec::new();
        let mut rest = after_bi;
        let data_start = loop {
            rest = skip_content_whitespace(rest);
            if rest.len() >= 2 && rest[0] == b'I' && rest[1] == b'D' && is_id_terminator(rest.get(2)) {
                break &rest[2..];
            }
            let (key, after_key) = take_inline_name(rest)
                .ok_or_else(|| "inline image missing ID operator".to_string())?;
            let after_key = skip_content_whitespace(after_key);
            let (value, after_value) = take_inline_value(after_key)
                .ok_or_else(|| "inline image has malformed dict value".to_string())?;
            dict.push((key, value));
            rest = after_value;
        };
        // lopdf requires exactly one whitespace byte after `ID`.
        let data = data_start.strip_prefix(b" ")
            .or_else(|| data_start.strip_prefix(b"\n"))
            .or_else(|| data_start.strip_prefix(b"\r"))
            .or_else(|| data_start.strip_prefix(b"\t"))
            .ok_or_else(|| "inline image ID not followed by whitespace".to_string())?;
        let expected = inline_image_data_len(&dict)?;
        let image_data = data.get(..expected).ok_or_else(|| {
            "inline image data shorter than /W /H /BPC imply".to_string()
        })?;
        // Guard the `EI`-in-data trap from the other direction too: image
        // data containing `EI`-like bytes must not confuse validation, since
        // the length — not a literal scan — determines where data ends.
        let _ = image_data;
        let after_data = &data[expected..];
        // lopdf reads `EI` as `(content_space, tag(b"EI"), content_space)`:
        // whitespace is required before `EI` (the data-length computation
        // consumes the separator) and at least one trailing whitespace byte
        // must follow for the parse to complete.
        let ei = after_data.strip_prefix(b" ")
            .or_else(|| after_data.strip_prefix(b"\n"))
            .or_else(|| after_data.strip_prefix(b"\r"))
            .or_else(|| after_data.strip_prefix(b"\t"))
            .ok_or_else(|| "inline image data not followed by EI".to_string())?;
        if ei.len() < 2 || ei[0] != b'E' || ei[1] != b'I' {
            return Err("inline image data not followed by EI".to_string());
        }
        if ei.len() < 3 || !is_content_whitespace(ei[2]) {
            return Err("inline image EI not followed by whitespace".to_string());
        }
        cursor = &ei[2..];
    }
    Ok(())
}

/// Length of inline-image data implied by the inline dict entries.
pub(super) fn inline_image_data_len(dict: &[(&[u8], &[u8])]) -> Result<usize, String> {
    let lookup = |abbr: &[u8], full: &[u8]| -> Option<&[u8]> {
        dict.iter()
            .find(|(key, _)| *key == abbr || *key == full)
            .map(|(_, value)| *value)
    };
    let parse_int = |raw: &[u8]
| -> Option<i64> {
        std::str::from_utf8(raw).ok()?.trim().parse::<i64>().ok()
    };
    let width = lookup(b"W", b"Width")
        .and_then(parse_int)
        .ok_or_else(|| "inline image missing /W".to_string())?;
    let height = lookup(b"H", b"Height")
        .and_then(parse_int)
        .ok_or_else(|| "inline image missing /H".to_string())?;
    let bpc = lookup(b"BPC", b"BitsPerComponent")
        .and_then(parse_int)
        .ok_or_else(|| "inline image missing /BPC".to_string())?;
    if width <= 0 || height <= 0 || bpc <= 0 {
        return Err("inline image has non-positive /W /H /BPC".to_string());
    }
    if lookup(b"F", b"Filter").is_some() {
        return Err("filtered inline images are unsupported".to_string());
    }
    let is_mask = lookup(b"IM", b"ImageMask").is_some_and(|raw| raw == b"true");
    let components: usize = if is_mask {
        1
    } else {
        let cs = lookup(b"CS", b"ColorSpace")
            .ok_or_else(|| "inline image missing /CS".to_string())?;
        match cs {
            b"G" | b"DeviceGray" | b"Gray" => 1,
            b"RGB" | b"DeviceRGB" => 3,
            b"CMYK" | b"DeviceCMYK" => 4,
            _ => return Err("inline image has unsupported colorspace".to_string()),
        }
    };
    let width = width as usize;
    let height = height as usize;
    let bpc = bpc as usize;
    let row_bits = width.checked_mul(components.checked_mul(bpc).ok_or_else(|| {
        "inline image dimensions overflow".to_string()
    })?).ok_or_else(|| "inline image dimensions overflow".to_string())?;
    let stride = row_bits.div_ceil(8);
    height.checked_mul(stride).ok_or_else(|| {
        "inline image dimensions overflow".to_string()
    })
}

pub(super) fn is_content_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

pub(super) fn skip_content_whitespace(mut input: &[u8]) -> &[u8] {
    while input.first().is_some_and(|byte| is_content_whitespace(*byte)) {
        input = &input[1..];
    }
    input
}

/// `ID` ends the inline dict only when followed by whitespace; `ID` glued to
/// data (e.g. `ID\x00`) belongs to the data run, matching lopdf which parses
/// `pair(tag(b"ID"), content_space)`.
pub(super) fn is_id_terminator(next: Option<&u8>) -> bool {
    next.is_some_and(|byte| is_content_whitespace(*byte))
}

/// Find a `BI` operator token (not a prefix of a longer operator such as
/// `BIT`).
pub(super) fn find_inline_begin(content: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index + 2 <= content.len() {
        if content[index] == b'B' && content[index + 1] == b'I' {
            let prev_ok = index == 0 || is_content_whitespace(content[index - 1]);
            let next = content.get(index + 2);
            let next_ok = next.is_none_or(|byte| is_content_whitespace(*byte));
            if prev_ok && next_ok {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

/// Take a `/Name` (without the leading slash) from an inline dict.
pub(super) fn take_inline_name(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = input.strip_prefix(b"/")?;
    let end = rest
        .iter()
        .position(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'*' | b'\'' | b'"' | b'_' | b'.' | b'-'))?;
    if end == 0 {
        return None;
    }
    Some((&rest[..end], &rest[end..]))
}

/// Take one inline-dict value: boolean, number, `/Name`, `[array]`, or
/// `(string)`. Returns the raw bytes and the remainder.
pub(super) fn take_inline_value(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let first = *input.first()?;
    if first == b'/' {
        let (name, rest) = take_inline_name(input)?;
        // Reconstruct the `/Name` span length from the remainder pointers.
        let consumed = input.len() - rest.len();
        let _ = name;
        return Some((&input[1..consumed], rest));
    }
    if first == b'[' {
        let end = input.iter().position(|byte| *byte == b']')?;
        return Some((&input[..end + 1], &input[end + 1..]));
    }
    if first == b'(' {
        // Literal string with nesting/escapes.
        let mut depth = 0usize;
        let mut index = 0usize;
        let mut escaped = false;
        while index < input.len() {
            let byte = input[index];
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'(' {
                depth += 1;
            } else if byte == b')' {
                depth -= 1;
                if depth == 0 {
                    return Some((&input[..index + 1], &input[index + 1..]));
                }
            }
            index += 1;
        }
        return None;
    }
    // Boolean, number, or bare keyword: run to the next whitespace.
    let end = input
        .iter()
        .position(|byte| is_content_whitespace(*byte))
        .unwrap_or(input.len());
    if end == 0 {
        return None;
    }
    Some((&input[..end], &input[end..]))
}
