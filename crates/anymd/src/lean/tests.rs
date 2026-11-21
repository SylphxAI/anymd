use anymd_core::text_rules::yaml_value;
use rmcp::model::CallToolResult;

use crate::document::Opened;
use crate::schema::{ReadArgs, SearchArgs};
use crate::source_access::SourceAccessPolicy;

use super::read::front_matter;
use super::read::pagination::{cut_point, describe_pages, read_opened, SourceRead};
use super::search::matching::{fold, snippet, terms_of};
use super::{estimate_tokens, read, search, Cursor};

fn read_fixture(name: &str, pages: Option<Vec<u32>>) -> SourceRead {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name)
        .display()
        .to_string();
    let options = crate::document::OpenOptions {
        ocr: Some(false),
        ..Default::default()
    };
    let mut opened = Opened::open(
        &path,
        &crate::source_access::SourceAccessPolicy::unrestricted(),
        &options,
    )
    .unwrap();
    read_opened(&mut opened, pages, None, usize::MAX, true, None)
}

#[test]
fn a_scanned_page_gets_a_marker_and_a_header_field() {
    let read = read_fixture("scanned-page.pdf", None);
    assert!(
        read.body.contains(
            "<!-- page 1 -->\n\n<!-- page 1: scanned image, no text layer; to enable local OCR, run `anymd setup ocr` (opts into downloading the OCR engine and ~2 GB of model weights), then retry with OCR enabled (`ocr: true` in MCP or `--ocr auto` in the CLI); documents stay on this machine -->"
        ),
        "{}",
        read.body
    );
    assert!(!read.body.contains("page 2: scanned"), "{}", read.body);
    assert!(read.body.contains("real text layer"));
    let field = read.header.iter().find(|(k, _)| k == "scanned_pages");
    assert_eq!(field.map(|(_, v)| v.as_str()), Some("[1]"));
    // One page of two is text, so there is no all-scans hint.
    assert!(!read.body.starts_with("<!-- Every page"), "{}", read.body);
    assert!(front_matter(&read.header).contains("scanned_pages: [1]\n"));
}

#[test]
fn a_scanned_page_where_ocr_ran_says_it_found_no_text() {
    let mut opened = Opened::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/scanned-page.pdf")
            .display()
            .to_string(),
        &crate::source_access::SourceAccessPolicy::unrestricted(),
        &crate::document::OpenOptions {
            ocr: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    opened.note_ocr_ran(1);
    let read = read_opened(&mut opened, Some(vec![1]), None, usize::MAX, true, None);
    assert!(
        read.body
            .contains("<!-- page 1: scanned image; OCR found no text -->"),
        "{}",
        read.body
    );
    assert!(!read.body.contains("enable OCR"), "{}", read.body);
}

#[test]
fn all_scanned_pages_lead_with_a_hint() {
    let read = read_fixture("scanned-page.pdf", Some(vec![1]));
    assert!(
        read.body
            .starts_with("<!-- Every page shown is a scanned image"),
        "{}",
        read.body
    );
}

/// A PDF of `pages` full-page image-only pages sharing one tiny image.
fn scan_pdf(pages: u32) -> Vec<u8> {
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            (0..pages)
                .map(|i| format!("{} 0 R", 6 + i))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nA\nendstream".into(),
        "<< /Length 30 >>\nstream\nq 612 0 0 792 0 0 cm /Im1 Do Q\nendstream".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    for _ in 0..pages {
        objects.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 3 0 R >> >> /Contents 4 0 R >>".into());
    }
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

#[test]
fn a_long_scan_stays_within_the_token_budget_and_returns_a_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scan.pdf");
    std::fs::write(&path, scan_pdf(500)).unwrap();
    let options = crate::document::OpenOptions {
        ocr: Some(false),
        ..Default::default()
    };
    let mut opened = Opened::open(
        &path.display().to_string(),
        &crate::source_access::SourceAccessPolicy::unrestricted(),
        &options,
    )
    .unwrap();
    let read = read_opened(&mut opened, None, None, 20_000, true, None);
    assert!(read.next.is_some(), "expected a cursor");
    let header = front_matter(&read.header);
    let total = estimate_tokens(&header) + estimate_tokens(&read.body);
    assert!(total <= 20_000, "{total} tokens");
    assert!(read.header.iter().any(|(k, _)| k == "scanned_pages"));
    assert!(!read.body.contains("Install `tesseract`"), "{}", read.body);
}

#[test]
fn text_pdfs_get_no_scan_marker_or_field() {
    for name in ["sample.pdf", "figure-report.pdf"] {
        let read = read_fixture(name, None);
        assert!(
            !read.body.contains("scanned image"),
            "{name}: {}",
            read.body
        );
        assert!(read.header.iter().all(|(k, _)| k != "scanned_pages"));
    }
}

#[test]
fn cursor_round_trips() {
    assert_eq!(Cursor::parse("7").unwrap(), Cursor { page: 7, offset: 0 });
    assert_eq!(
        Cursor::parse("3:1200").unwrap(),
        Cursor {
            page: 3,
            offset: 1200
        }
    );
    assert_eq!(Cursor::parse("3:1200").unwrap().render(), "3:1200");
    assert!(Cursor::parse("0").is_err());
    assert!(Cursor::parse("x").is_err());
}

#[test]
fn estimates_tokens_conservatively() {
    assert_eq!(estimate_tokens("hello world"), 2);
    assert_eq!(estimate_tokens("12345"), 2);
    assert_eq!(estimate_tokens("|a|"), 3);
    assert_eq!(estimate_tokens("注意力"), 3);
}

#[test]
fn describes_page_runs() {
    assert_eq!(describe_pages(&[1, 2, 3, 5, 7, 8]), "1-3,5,7-8");
}

#[test]
fn cuts_at_paragraph_boundaries() {
    let text = "aaaa\n\nbbbb\n\ncccc";
    assert_eq!(&text[..cut_point(text, 12)], "aaaa\n\nbbbb\n\n");
}

#[test]
fn snippet_bolds_the_match() {
    let text = "The Transformer uses multi-head attention everywhere.";
    let start = text.find("multi-head").unwrap();
    let end = start + "multi-head attention".len();
    let s = snippet(text, start, end, 10, &[(start, end)]);
    assert!(s.contains("**multi-head attention**"), "{s}");
}

#[test]
fn folding_maps_back_to_source_offsets() {
    let (folded, map) = fold("Multi-Head\n  Attention", false);
    assert_eq!(folded, "multi-head attention");
    assert_eq!(map[11], 13);
}

#[test]
fn terms_split_words_and_cjk() {
    assert_eq!(
        terms_of("Self-Attention 注意"),
        ["self", "attention", "注", "意"]
    );
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

fn fixture_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.md"),
        "# Notes\n\nThe transformer uses multi-head attention.\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("prices.csv"),
        "item,price\napple,1.20\npear,0.95\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        "<html><head><title>Guide</title></head><body><main><h1>Guide</h1><p>Attention heads attend to tokens.</p></main></body></html>",
    )
    .unwrap();
    dir
}

#[test]
fn read_converts_a_csv_to_a_table() {
    let dir = fixture_dir();
    let args = ReadArgs {
        timeline: None,
        source: dir.path().join("prices.csv").display().to_string(),
        pages: None,
        node: None,
        max_tokens: None,
        cursor: None,
        ocr: None,
        transcript: None,
        download_asr_model: None,
        images: None,
        revisions: None,
    };
    let text = text_of(&read(&args, &SourceAccessPolicy::unrestricted()).unwrap());
    assert!(text.contains("format: csv"), "{text}");
    assert!(text.contains("|apple|1.20|"), "{text}");
}

#[test]
fn default_budget_still_pages_a_large_converted_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.html");
    std::fs::write(
        &path,
        format!(
            "<html><body><p>{}</p></body></html>",
            "chapter text ".repeat(100_000)
        ),
    )
    .unwrap();
    let mut args: ReadArgs =
        serde_json::from_value(serde_json::json!({ "source": path.display().to_string() }))
            .unwrap();
    let policy = SourceAccessPolicy::unrestricted();
    let result = read(&args, &policy).unwrap();
    assert_ne!(result.is_error, Some(true));
    let first = text_of(&result);
    assert!(first.contains("Stopped at the 20000-token budget"));
    let cursor = first
        .split("Continue with cursor: \"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    args.cursor = Some(cursor.to_string());
    let result = read(&args, &policy).unwrap();
    assert_ne!(result.is_error, Some(true));
    let second = text_of(&result);
    assert!(second.contains("chapter text"));
    let next = second
        .split("Continue with cursor: \"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert_ne!(cursor, next);
}

#[test]
fn read_lists_a_directory() {
    let dir = fixture_dir();
    let args = ReadArgs {
        timeline: None,
        source: dir.path().display().to_string(),
        pages: None,
        node: None,
        max_tokens: None,
        cursor: None,
        ocr: None,
        transcript: None,
        download_asr_model: None,
        images: None,
        revisions: None,
    };
    let text = text_of(&read(&args, &SourceAccessPolicy::unrestricted()).unwrap());
    assert!(text.contains("readable_files: 3"), "{text}");
    assert!(text.contains("- notes.md"), "{text}");
}

#[test]
fn search_finds_literal_hits_across_formats_then_ranks() {
    let dir = fixture_dir();
    let mut args = SearchArgs {
        query: "multi-head attention".into(),
        sources: vec![dir.path().display().to_string()],
        mode: None,
        glob: None,
        case_sensitive: None,
        whole_word: None,
        max_results: None,
        context_chars: None,
    };
    let policy = SourceAccessPolicy::unrestricted();
    let text = text_of(&search(&args, &policy).unwrap());
    assert!(
        text.starts_with("1 match for \"multi-head attention\" (3 files"),
        "{text}"
    );
    assert!(text.contains("**multi-head attention**"), "{text}");

    args.query = "attention tokens heads".into();
    let text = text_of(&search(&args, &policy).unwrap());
    assert!(text.contains("No exact match"), "{text}");
    assert!(text.contains("page.html"), "{text}");

    args.query = "pear".into();
    args.glob = Some("*.csv".into());
    let text = text_of(&search(&args, &policy).unwrap());
    assert!(text.contains("1 file"), "{text}");
}

#[test]
fn yaml_values_quote_when_needed() {
    assert_eq!(
        yaml_value("Attention Is All You Need"),
        "Attention Is All You Need"
    );
    assert_eq!(yaml_value("BERT: Pre-training"), "\"BERT: Pre-training\"");
}
