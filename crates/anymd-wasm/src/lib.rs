//! anymd compiled to WebAssembly: bytes + a file name in, clean Markdown out.
//!
//! Output follows the CLI conventions: a small front-matter header (source,
//! format, title, pages/slides/sheets) and `<!-- page N -->` style markers on
//! paged documents. Everything runs in the caller's process: in a browser the
//! file never leaves the page.

use anymd_core::document::{open_bytes, OpenBytesOptions, OpenError, OpenedDocument};
use wasm_bindgen::prelude::*;

/// Convert a file to Markdown with front matter and unit markers.
#[wasm_bindgen]
pub fn convert(bytes: &[u8], filename: &str) -> Result<String, JsError> {
    convert_markdown(bytes, filename).map_err(|message| JsError::new(&message))
}

/// Like `convert`, but a JSON string with the Markdown and a few facts:
/// `{ markdown, format, title, units, unit_noun, tokens, source_units }`.
#[wasm_bindgen(js_name = convertDetailed)]
pub fn convert_detailed(bytes: &[u8], filename: &str) -> Result<String, JsError> {
    let document = open_for_wasm(bytes, filename).map_err(|message| JsError::new(&message))?;
    let markdown = document.render_markdown(filename);
    let value = serde_json::json!({
        "format": document.format,
        "title": document.title,
        "units": document.units.len(),
        "unit_noun": document.unit_noun,
        "tokens": estimate_tokens(&markdown),
        "markdown": markdown,
        "source_units": document.units.iter().map(|unit| serde_json::json!({"label": unit.label, "text": unit.markdown})).collect::<Vec<_>>(),
    });
    Ok(value.to_string())
}

/// The Markdown's token estimate: the same rule the CLI applies.
#[wasm_bindgen(js_name = estimateTokens)]
pub fn estimate_tokens(text: &str) -> usize {
    anymd_core::text_rules::estimate_tokens(text)
}

/// anymd version this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Rust-side entry (also used by native tests).
pub fn convert_markdown(bytes: &[u8], filename: &str) -> Result<String, String> {
    let document = open_for_wasm(bytes, filename)?;
    Ok(document.render_markdown(filename))
}

fn open_for_wasm(bytes: &[u8], filename: &str) -> Result<OpenedDocument, String> {
    open_bytes(
        bytes,
        &OpenBytesOptions {
            filename: Some(filename.into()),
            ..Default::default()
        },
    )
    .map_err(|error| match error {
        // Keep the browser's existing converter and PDF diagnostics.
        OpenError::PdfParse(source) => source.to_string(),
        OpenError::Conversion { source, .. } => source.to_string(),
        error => error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_gets_front_matter_without_markers() {
        let html = b"<!doctype html><html><head><title>Hi</title></head><body><h1>Hello</h1><p>World</p></body></html>";
        let out = convert_markdown(html, "page.html").unwrap();
        assert!(
            out.starts_with("---\nsource: page.html\nformat: html\n"),
            "{out}"
        );
        assert!(out.contains("Hello"), "{out}");
        assert!(!out.contains("<!-- "), "{out}");
    }

    #[test]
    fn detailed_conversion_keeps_the_browser_response_shape() {
        let bytes = "# 中文\n\nRésumé 😀\n".as_bytes();
        let markdown =
            "---\nsource: note.md\nformat: text\ntitle: 中文\n---\n\n# 中文\n\nRésumé 😀\n";
        let detailed = convert_detailed(bytes, "note.md").unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&detailed).unwrap(),
            serde_json::json!({
                "format": "text", "title": "中文", "units": 1, "unit_noun": "section",
                "tokens": estimate_tokens(markdown), "markdown": markdown,
                "source_units": [{"label": "document", "text": "# 中文\n\nRésumé 😀\n"}]
            })
        );
    }

    #[test]
    fn csv_is_a_table() {
        let out = convert_markdown(b"a,b\n1,2\n", "t.csv").unwrap();
        assert!(out.contains("|a|b|"), "{out}");
    }

    #[test]
    fn delimited_encodings_and_errors_reach_the_browser_entry() {
        for (name, delimiter) in [("t.csv", ','), ("t.tsv", '\t')] {
            let text = format!(
                "name{delimiter}city\r\nAlice{delimiter}Hong Kong\r\n陳小明😀{delimiter}香港\r\n"
            );
            let expected = convert_markdown(text.as_bytes(), name).unwrap();
            assert!(expected.contains("|name|city|\n|-|-|\n|Alice|Hong Kong|\n|陳小明😀|香港|\n"));
            let mut encodings = vec![format!("\u{feff}{text}").into_bytes()];
            for little_endian in [true, false] {
                let mut bytes = Vec::new();
                for unit in format!("\u{feff}{text}").encode_utf16() {
                    bytes.extend(if little_endian {
                        unit.to_le_bytes()
                    } else {
                        unit.to_be_bytes()
                    });
                }
                encodings.push(bytes);
            }
            for bytes in encodings {
                assert_eq!(convert_markdown(&bytes, name).unwrap(), expected);
            }
            for bytes in [&[0xFF, 0xFE, 0x41][..], &[0xFE, 0xFF, 0xD8, 0x00][..]] {
                assert!(convert_markdown(bytes, name)
                    .unwrap_err()
                    .contains("re-export"));
                assert_eq!(convert_markdown(text.as_bytes(), name).unwrap(), expected);
            }
        }
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(convert_markdown(&[0u8, 159, 146, 150, 0, 1], "x.bin").is_err());
    }

    #[test]
    fn token_estimate_matches_cli_heuristic() {
        assert_eq!(estimate_tokens("hello world"), 2);
        assert_eq!(estimate_tokens("12345"), 2);
    }
}
