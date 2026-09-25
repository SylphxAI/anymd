//! anymd compiled to WebAssembly: bytes + a file name in, clean Markdown out.
//!
//! Output follows the CLI conventions: a small front-matter header (source,
//! format, title, pages/slides/sheets) and `<!-- page N -->` style markers on
//! paged documents. Everything runs in the caller's process: in a browser the
//! file never leaves the page.

use std::path::Path;

use anymd_formats::{Converted, Format, Options};
use wasm_bindgen::prelude::*;

/// PDF pages converted per pass (the CLI's chunk size).
const PAGE_CHUNK: usize = 12;

/// One citable unit (page, slide, sheet, chapter).
struct Unit {
    label: String,
    markdown: String,
}

struct Document {
    format: &'static str,
    title: Option<String>,
    metadata: Vec<(String, String)>,
    units: Vec<Unit>,
    paged: bool,
}

/// Convert a file to Markdown with front matter and unit markers.
#[wasm_bindgen]
pub fn convert(bytes: &[u8], filename: &str) -> Result<String, JsError> {
    convert_markdown(bytes, filename).map_err(|message| JsError::new(&message))
}

/// Like `convert`, but a JSON string with the Markdown and a few facts:
/// `{ markdown, format, title, units, unit_noun, tokens }`.
#[wasm_bindgen(js_name = convertDetailed)]
pub fn convert_detailed(bytes: &[u8], filename: &str) -> Result<String, JsError> {
    let document = open(bytes, filename).map_err(|message| JsError::new(&message))?;
    let markdown = render(&document, filename);
    let value = serde_json::json!({
        "format": document.format,
        "title": document.title,
        "units": document.units.len(),
        "unit_noun": noun(&document),
        "tokens": estimate_tokens(&markdown),
        "markdown": markdown,
    });
    Ok(value.to_string())
}

/// Cheap, slightly conservative LLM token estimate (the CLI's heuristic).
#[wasm_bindgen(js_name = estimateTokens)]
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let mut run_of = |pred: fn(&char) -> bool| {
            let mut run: usize = 1;
            while chars.peek().is_some_and(pred) {
                chars.next();
                run += 1;
            }
            run
        };
        if ch.is_ascii_alphabetic() {
            tokens += run_of(char::is_ascii_alphabetic).div_ceil(5);
        } else if ch.is_ascii_digit() {
            tokens += run_of(char::is_ascii_digit).div_ceil(3);
        } else if ch.is_whitespace() {
            tokens += usize::from(run_of(|c| c.is_whitespace()) >= 3);
        } else {
            tokens += 1;
        }
    }
    tokens
}

/// anymd version this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Rust-side entry (also used by native tests).
pub fn convert_markdown(bytes: &[u8], filename: &str) -> Result<String, String> {
    let document = open(bytes, filename)?;
    Ok(render(&document, filename))
}

fn open(bytes: &[u8], filename: &str) -> Result<Document, String> {
    let path = (!filename.is_empty()).then(|| Path::new(filename));
    let head = &bytes[..bytes.len().min(8192)];
    let format = anymd_formats::detect(path, head).ok_or_else(|| {
        "Unrecognized file type (binary data with no known signature).".to_string()
    })?;
    let mut document = if format == Format::Pdf {
        open_pdf(bytes)?
    } else {
        let converted = anymd_formats::convert(format, bytes, &Options::default())
            .map_err(|error| error.to_string())?;
        from_converted(format, converted)
    };
    // No declared title: the first `# ` heading of unit 1 (as the CLI does).
    if document.title.is_none() {
        document.title = document.units.first().and_then(|unit| {
            unit.markdown
                .lines()
                .find_map(|line| line.strip_prefix("# "))
                .map(|title| title.trim().to_string())
        });
    }
    Ok(document)
}

fn open_pdf(bytes: &[u8]) -> Result<Document, String> {
    let doc = anymd_pdf::load_document_bytes(bytes).map_err(|error| error.message)?;
    let total = anymd_pdf::page_count(&doc);
    // Same 12-page chunks as the CLI, so heading levels match its output.
    let numbers: Vec<u32> = (1..=total).collect();
    let mut units = Vec::with_capacity(numbers.len());
    for chunk in numbers.chunks(PAGE_CHUNK) {
        let converted =
            anymd_pdf::pdf_to_markdown(&doc, Some(chunk)).map_err(|error| error.message)?;
        units.extend(converted.pages.into_iter().map(|page| Unit {
            label: format!("page {}", page.number),
            markdown: page.markdown,
        }));
    }
    Ok(Document {
        format: "pdf",
        title: anymd_pdf::info_title(&doc),
        metadata: Vec::new(),
        units,
        paged: true,
    })
}

fn from_converted(format: Format, converted: Converted) -> Document {
    Document {
        format: format.name(),
        title: converted.title,
        metadata: converted.metadata,
        units: converted
            .sections
            .into_iter()
            .map(|section| Unit {
                label: section.label,
                markdown: section.markdown,
            })
            .collect(),
        paged: false,
    }
}

fn noun(document: &Document) -> &'static str {
    let first_word = |unit: &Unit| {
        unit.label
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()
    };
    let shared = document
        .units
        .first()
        .map(first_word)
        .filter(|word| document.units.iter().all(|unit| first_word(unit) == *word));
    match shared.as_deref() {
        Some("slide") => "slide",
        Some("sheet") => "sheet",
        Some("chapter") => "chapter",
        _ if document.paged => "page",
        _ => "section",
    }
}

fn render(document: &Document, filename: &str) -> String {
    let total = document.units.len();
    // One-unit documents (a web page, a DOCX, an image) need no unit markers.
    let markers = total > 1 || document.paged;
    let mut body = String::new();
    for unit in &document.units {
        if markers {
            body.push_str(&format!("<!-- {} -->\n\n", unit.label));
        }
        body.push_str(unit.markdown.trim());
        body.push_str("\n\n");
    }
    if document.paged && total > 0 {
        let visible: usize = document
            .units
            .iter()
            .map(|unit| {
                unit.markdown
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .count()
            })
            .sum();
        if visible < total * 20 {
            body.push_str(
                "<!-- These pages have little or no selectable text (scanned or image-only). \
The anymd CLI can OCR them with a local `tesseract`. -->\n\n",
            );
        }
    }

    let label = if filename.is_empty() {
        "input"
    } else {
        filename
    };
    let mut header = vec![("source".to_string(), label.to_string())];
    if document.format != "pdf" {
        header.push(("format".into(), document.format.to_string()));
    }
    if let Some(title) = &document.title {
        header.push(("title".into(), title.clone()));
    }
    for (key, value) in document.metadata.iter().take(6) {
        if key != "title" && key != "format" && !value.is_empty() && value.len() <= 200 {
            header.push((key.replace(' ', "_"), value.clone()));
        }
    }
    if markers {
        let key = format!("{}s", noun(document));
        header.retain(|(existing, _)| *existing != key);
        header.push((key, total.to_string()));
    }

    let mut out = String::from("---\n");
    for (key, value) in &header {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(&yaml_value(value));
        out.push('\n');
    }
    out.push_str("---\n\n");
    out.push_str(&body);
    out.trim_end().to_string() + "\n"
}

fn yaml_value(value: &str) -> String {
    let single_line = value.replace(['\r', '\n'], " ");
    let needs_quotes = single_line.contains(": ")
        || single_line.starts_with([
            '"', '\'', '[', '{', '#', '&', '*', '!', '|', '>', '%', '@', '`', '-', '?',
        ])
        || single_line.ends_with(':');
    if needs_quotes {
        serde_json::to_string(&single_line).unwrap_or(single_line)
    } else {
        single_line
    }
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
    fn csv_is_a_table() {
        let out = convert_markdown(b"a,b\n1,2\n", "t.csv").unwrap();
        assert!(out.contains("|a|b|"), "{out}");
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
