extern crate pom;

use pom::char_class::{alpha, hex_digit, multispace, oct_digit};
use pom::parser::*;
use std::collections::HashMap;

use std::str::FromStr;

#[derive(Debug)]
pub enum Value {
    LiteralString(Vec<u8>),
    Name(Vec<u8>),
    Number(String),
    Integer(i64),
    Array(Vec<Value>),
    Operator(String),
    Boolean(bool),
    Dictionary(HashMap<String, Value>),
}

fn hex_char<'a>() -> Parser<'a, u8, u8> {
    let number = is_a(hex_digit).repeat(2);
    number
        .collect()
        .convert(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16))
}

fn comment<'a>() -> Parser<'a, u8, ()> {
    sym(b'%') * none_of(b"\r\n").repeat(0..) * eol().discard()
}

fn content_space<'a>() -> Parser<'a, u8, ()> {
    is_a(multispace).repeat(0..).discard()
}

fn operator<'a>() -> Parser<'a, u8, String> {
    (is_a(alpha) | one_of(b"*'\""))
        .repeat(1..)
        .convert(String::from_utf8)
}

fn oct_char<'a>() -> Parser<'a, u8, u8> {
    let number = is_a(oct_digit).repeat(1..4);
    number
        .collect()
        .convert(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 8))
}

fn escape_sequence<'a>() -> Parser<'a, u8, Vec<u8>> {
    sym(b'\\')
        * (sym(b'\\').map(|_| vec![b'\\'])
            | sym(b'(').map(|_| vec![b'('])
            | sym(b')').map(|_| vec![b')'])
            | sym(b'n').map(|_| vec![b'\n'])
            | sym(b'r').map(|_| vec![b'\r'])
            | sym(b't').map(|_| vec![b'\t'])
            | sym(b'b').map(|_| vec![b'\x08'])
            | sym(b'f').map(|_| vec![b'\x0C'])
            | oct_char().map(|c| vec![c])
            | eol().map(|_| vec![])
            | empty().map(|_| vec![]))
}

fn nested_literal_string<'a>() -> Parser<'a, u8, Vec<u8>> {
    sym(b'(')
        * (none_of(b"\\()").repeat(1..) | escape_sequence() | call(nested_literal_string))
            .repeat(0..)
            .map(|segments| {
                let mut bytes = segments
                    .into_iter()
                    .fold(vec![b'('], |mut bytes, mut segment| {
                        bytes.append(&mut segment);
                        bytes
                    });
                bytes.push(b')');
                bytes
            })
        - sym(b')')
}

fn literal_string<'a>() -> Parser<'a, u8, Vec<u8>> {
    sym(b'(')
        * (none_of(b"\\()").repeat(1..) | escape_sequence() | nested_literal_string())
            .repeat(0..)
            .map(|segments| segments.concat())
        - sym(b')')
}

fn name<'a>() -> Parser<'a, u8, Vec<u8>> {
    sym(b'/') * (none_of(b" \t\n\r\x0C()<>[]{}/%#") | (sym(b'#') * hex_char())).repeat(0..)
}

fn integer<'a>() -> Parser<'a, u8, i64> {
    let number = one_of(b"+-").opt() + one_of(b"0123456789").repeat(1..);
    number
        .collect()
        .convert(std::str::from_utf8)
        .convert(i64::from_str)
}

fn number<'a>() -> Parser<'a, u8, String> {
    let number = one_of(b"+-").opt()
        + ((one_of(b"0123456789") - one_of(b"0123456789").repeat(0..).discard())
            | (one_of(b"0123456789").repeat(1..) * sym(b'.') - one_of(b"0123456789").repeat(0..))
            | (sym(b'.') - one_of(b"0123456789").repeat(1..)));
    number
        .collect()
        .convert(std::str::from_utf8)
        .map(str::to_owned)
}

fn space<'a>() -> Parser<'a, u8, ()> {
    (one_of(b" \t\n\r\0\x0C").repeat(1..).discard())
        .repeat(0..)
        .discard()
}

// Dictionaries are not mentioned in the CMap spec but are produced by software like Cairo and Skia and supported other by readers
fn dictionary<'a>() -> Parser<'a, u8, HashMap<String, Value>> {
    let entry = name() - space() + call(value);
    let entries = seq(b"<<") * space() * entry.repeat(0..) - seq(b">>");
    entries.map(|entries| {
        entries.into_iter().fold(
            HashMap::new(),
            |mut dict: HashMap<String, Value>, (key, value)| {
                dict.insert(String::from_utf8(key).unwrap(), value);
                dict
            },
        )
    })
}

fn hexadecimal_string<'a>() -> Parser<'a, u8, Vec<u8>> {
    sym(b'<') * (space() * hex_char()).repeat(0..) - (space() * sym(b'>'))
}

fn eol<'a>() -> Parser<'a, u8, u8> {
    (sym(b'\r') * sym(b'\n')) | sym(b'\n') | sym(b'\r')
}

fn value<'a>() -> Parser<'a, u8, Value> {
    (seq(b"true").map(|_| Value::Boolean(true))
        | seq(b"false").map(|_| Value::Boolean(false))
        | integer().map(Value::Integer)
        | number().map(Value::Number)
        | name().map(Value::Name)
        | operator().map(Value::Operator)
        | literal_string().map(Value::LiteralString)
        | dictionary().map(Value::Dictionary)
        | hexadecimal_string().map(Value::LiteralString)
        | array().map(Value::Array))
        - content_space()
}

fn array<'a>() -> Parser<'a, u8, Vec<Value>> {
    sym(b'[') * space() * call(value).repeat(0..) - sym(b']')
}

fn file<'a>() -> Parser<'a, u8, Vec<Value>> {
    (comment().repeat(0..) * content_space() * value()).repeat(1..)
}

pub fn parse(input: &[u8]) -> Result<Vec<Value>, pom::Error> {
    file().parse(input)
}

fn as_code(str: &[u8]) -> u32 {
    let mut code: u32 = 0;
    for c in str {
        code = (code << 8) | (*c as u32);
    }
    code
}

// Wide variant that does not truncate past 4 bytes; used for Unicode destination
// values in beginbfrange so multi-code (6/8-byte) destinations are preserved.
fn as_code_wide(str: &[u8]) -> u128 {
    let mut code: u128 = 0;
    for c in str {
        code = (code << 8) | (*c as u128);
    }
    code
}

/// Return a mapping from character codes to Unicode character sequences expressed in UTF-16BE encoding.
///
/// This vendored copy diverges from upstream `adobe-cmap-parser` only in that it is
/// *infallible on malformed input*: instead of `panic!`/index-panics on unusual or
/// broken CMap destinations (for example 1-byte `beginbfrange` destinations or
/// odd-length UTF-16BE values emitted by some TeX/pdfTeX fonts), it skips the
/// unusable entries. Odd-length destinations cannot be valid UTF-16BE and are dropped.
/// The upstream crate panicked with "bad length of hexstring", which aborted the whole
/// pdf-reader-mcp server process.
pub fn get_unicode_map(input: &[u8]) -> Result<HashMap<u32, Vec<u8>>, &'static str> {
    let lexed = match parse(input) {
        Ok(lexed) => lexed,
        Err(_) => return Ok(HashMap::new()),
    };

    let mut i = 0;
    let mut map = HashMap::new();
    while i < lexed.len() {
        match lexed[i] {
            Value::Operator(ref o) => {
                match o.as_ref() {
                    "beginbfchar" => {
                        let Some(Value::Integer(count)) = lexed.get(i.wrapping_sub(1)) else {
                            i += 1;
                            continue;
                        };
                        i += 1;
                        let mut handled = 0usize;
                        while handled < *count as usize {
                            let (
                                Some(Value::LiteralString(char_code)),
                                Some(Value::LiteralString(uni_code)),
                            ) = (lexed.get(i), lexed.get(i + 1))
                            else {
                                break;
                            };
                            // Only even-length destinations are valid UTF-16BE; skip the rest.
                            if uni_code.len() % 2 == 0 {
                                map.insert(as_code(char_code), uni_code.clone());
                            }
                            // Advance even when skipping so later pairs stay aligned.
                            i += 2;
                            handled += 1;
                        }
                        i += 1;
                    }
                    "beginbfrange" => {
                        let Some(Value::Integer(count)) = lexed.get(i.wrapping_sub(1)) else {
                            i += 1;
                            continue;
                        };
                        i += 1;
                        let mut handled = 0usize;
                        while handled < *count as usize {
                            let (
                                Some(Value::LiteralString(lower)),
                                Some(Value::LiteralString(upper)),
                            ) = (lexed.get(i), lexed.get(i + 1))
                            else {
                                break;
                            };
                            let lower_code = as_code(lower);
                            let upper_code = as_code(upper);
                            match lexed.get(i + 2) {
                                Some(Value::LiteralString(start)) => {
                                    if start.len() % 2 == 0 {
                                        let width = start.len();
                                        let base = as_code_wide(start);
                                        for c in lower_code..=upper_code {
                                            let offset = (c - lower_code) as u128;
                                            let value = base.wrapping_add(offset);
                                            let bytes = value.to_be_bytes();
                                            let first = bytes.len() - width;
                                            map.insert(c, bytes[first..].to_vec());
                                        }
                                    }
                                    // Odd-length destinations cannot be UTF-16BE; skip.
                                }
                                Some(Value::Array(codes)) => {
                                    let mut ci = 0usize;
                                    while ci < codes.len() {
                                        if let Some(Value::LiteralString(s)) = codes.get(ci) {
                                            if s.len() % 2 == 0 {
                                                map.insert(lower_code + ci as u32, s.clone());
                                            }
                                        }
                                        ci += 1;
                                    }
                                }
                                _ => {
                                    // Malformed third element; skip this range.
                                }
                            }
                            i += 3;
                            handled += 1;
                        }
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    Ok(map)
}

fn as_code_range(start_chars: &[u8], end_chars: &[u8]) -> Option<CodeRange> {
    // Upstream asserted equal lengths (a panic on malformed input); be lenient instead.
    if start_chars.len() != end_chars.len() {
        return None;
    }
    let mut start = 0;
    let mut end = 0;
    let width = start_chars.len();
    for i in 0..width {
        start = (start << 8) | (start_chars[i] as u32);
        end = (end << 8) | (end_chars[i] as u32);
    }
    Some(CodeRange {
        start,
        end,
        width: width as u32 / 2,
    })
}

#[derive(Debug)]
pub struct CodeRange {
    pub width: u32,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug)]
#[allow(non_snake_case)]
pub struct CIDRange {
    pub src_code_lo: u32,
    pub src_code_hi: u32,
    pub dst_CID_lo: u32,
}

#[derive(Debug)]
pub struct ByteMapping {
    pub codespace: Vec<CodeRange>,
    pub cid: Vec<CIDRange>,
}

pub fn get_byte_mapping(input: &[u8]) -> Result<ByteMapping, &'static str> {
    let lexed = match parse(input) {
        Ok(lexed) => lexed,
        Err(_) => {
            return Ok(ByteMapping {
                codespace: Vec::new(),
                cid: Vec::new(),
            })
        }
    };

    let mut i = 0;
    let mut result = ByteMapping {
        codespace: Vec::new(),
        cid: Vec::new(),
    };
    while i < lexed.len() {
        match lexed[i] {
            Value::Operator(ref o) => match o.as_ref() {
                "begincodespacerange" => {
                    let Some(Value::Integer(count)) = lexed.get(i.wrapping_sub(1)) else {
                        i += 1;
                        continue;
                    };
                    i += 1;
                    let mut handled = 0usize;
                    while handled < *count as usize {
                        let (Some(Value::LiteralString(start)), Some(Value::LiteralString(end))) =
                            (lexed.get(i), lexed.get(i + 1))
                        else {
                            break;
                        };
                        if let Some(range) = as_code_range(start, end) {
                            result.codespace.push(range);
                        }
                        i += 2;
                        handled += 1;
                    }
                    i += 1;
                }
                "begincidrange" => {
                    let Some(Value::Integer(count)) = lexed.get(i.wrapping_sub(1)) else {
                        i += 1;
                        continue;
                    };
                    i += 1;
                    let mut handled = 0usize;
                    while handled < *count as usize {
                        let (Some(Value::LiteralString(start)), Some(Value::LiteralString(end))) =
                            (lexed.get(i), lexed.get(i + 1))
                        else {
                            break;
                        };
                        let offset = match lexed.get(i + 2) {
                            Some(Value::Integer(offset)) => *offset as u32,
                            _ => break,
                        };
                        result.cid.push(CIDRange {
                            src_code_lo: as_code(start),
                            src_code_hi: as_code(end),
                            dst_CID_lo: offset,
                        });
                        i += 2;
                        handled += 1;
                    }
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            },
            _ => {
                i += 1;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{get_byte_mapping, get_unicode_map};

    #[test]
    fn bfrange_odd_length_destination_does_not_panic() {
        // Regression for SylphxAI/pdf-reader-mcp#608: pdfTeX emits 1-byte
        // beginbfrange destinations like <C5> <D6> <C5>. Upstream panicked
        // with "bad length of hexstring"; we must skip it instead.
        let cmap = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/cmapname /TeX-MI-H def
1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfrange
<C5> <D6> <C5>
endbfrange
endcmap
CMapName currentdict /cmapname defineresource pop
end
end";
        let map = get_unicode_map(cmap).expect("should not panic");
        assert!(
            !map.contains_key(&0xC5),
            "odd-length bfrange destination must be skipped"
        );
    }

    #[test]
    fn bfrange_even_wide_destination_preserved() {
        // A 6-byte (3 UTF-16 code unit) destination is valid UTF-16BE and kept.
        let cmap = b"1 beginbfrange
<01> <01> <006600660069>
endbfrange";
        let map = get_unicode_map(cmap).expect("ok");
        assert_eq!(
            map.get(&0x01).map(Vec::as_slice),
            Some(&[0x00, 0x66, 0x00, 0x66, 0x00, 0x69][..])
        );
    }

    #[test]
    fn bfchar_odd_length_destination_skipped() {
        let cmap = b"1 beginbfchar
<00> <0FF>
endbfchar";
        let map = get_unicode_map(cmap).expect("ok");
        assert!(!map.contains_key(&0x00));
    }

    #[test]
    fn bfchar_six_byte_destination_kept() {
        // pdfTeX ligature destination <006600660069> == "ffi".
        let cmap = b"1 beginbfchar
<1E> <006600660069>
endbfchar";
        let map = get_unicode_map(cmap).expect("ok");
        assert_eq!(
            map.get(&0x1E).map(Vec::as_slice),
            Some(&[0x00, 0x66, 0x00, 0x66, 0x00, 0x69][..])
        );
    }

    #[test]
    fn parse_failure_returns_empty_map() {
        let map = get_unicode_map(b"\x00 not a cmap \xff\xfe").expect("no panic");
        assert!(map.is_empty());
    }

    #[test]
    fn byte_mapping_codespace_length_mismatch_no_panic() {
        let cmap = b"1 begincodespacerange
<00> <0000>
endcodespacerange";
        let mapping = get_byte_mapping(cmap).expect("no panic");
        assert!(mapping.codespace.is_empty());
    }
}
