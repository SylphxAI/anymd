//! Real-document regression for word spaces on the structured JSON path
//! (read_pdf full text, search_pdf matches), which extracts text through the
//! text index rather than the Markdown layout engine.
//!
//! Runs when ANYMD_CORPUS_DIR points at a directory filled by
//! scripts/fetch-markdown-corpus.sh; skips otherwise.

use pdf_reader_core::{read_pdf_from_value, search_pdf_from_value};
use serde_json::{json, Value};
use std::path::PathBuf;

fn corpus_file(name: &str) -> Option<String> {
    let Ok(dir) = std::env::var("ANYMD_CORPUS_DIR") else {
        eprintln!("ANYMD_CORPUS_DIR not set; skipping structured text corpus regression");
        return None;
    };
    Some(PathBuf::from(dir).join(name).display().to_string())
}

fn full_text(path: &str) -> String {
    let response = read_pdf_from_value(&json!({
        "sources": [{ "path": path }],
        "include_full_text": true,
    }))
    .unwrap_or_else(|error| panic!("read_pdf {path}: {}", error.message));
    let value = serde_json::to_value(response).expect("serialize read_pdf");
    value["results"][0]["data"]["full_text"]
        .as_str()
        .unwrap_or_else(|| panic!("no full_text in {value}"))
        .to_string()
}

#[test]
fn tex_paper_json_text_has_word_spaces() {
    let Some(path) = corpus_file("attention.pdf") else {
        return;
    };
    let text = full_text(&path);
    assert!(
        text.contains("The dominant sequence transduction models"),
        "full_text lost word spaces: {}",
        &text[..text.len().min(1200)]
    );
    for glued in [
        "Thedominantsequence",
        "transductionmodels",
        "multi-headattention",
    ] {
        assert!(!text.contains(glued), "full_text contains {glued:?}");
    }

    let response = search_pdf_from_value(&json!({
        "sources": [{ "path": path }],
        "query": "multi-head attention",
    }))
    .unwrap_or_else(|error| panic!("search_pdf: {}", error.message));
    let value = serde_json::to_value(response).expect("serialize search_pdf");
    let matches = value["results"][0]["matches"]
        .as_array()
        .unwrap_or_else(|| panic!("no matches array in {value}"));
    assert!(!matches.is_empty(), "no match for multi-head attention");
    for found in matches {
        assert!(found["page"].as_u64().is_some_and(|page| page >= 1));
        assert!(found["text"]
            .as_str()
            .is_some_and(|text| text.eq_ignore_ascii_case("multi-head attention")));
        let bbox: &Value = &found["bounding_box"];
        let (left, right) = (
            bbox["left"].as_f64().unwrap(),
            bbox["right"].as_f64().unwrap(),
        );
        let (bottom, top) = (
            bbox["bottom"].as_f64().unwrap(),
            bbox["top"].as_f64().unwrap(),
        );
        assert!(left < right && bottom < top, "degenerate match box {bbox}");
        // Two words at ~10pt: wider than one word, narrower than a line.
        assert!((40.0..200.0).contains(&(right - left)), "match box {bbox}");
    }
}

#[test]
fn cjk_json_text_stays_unspaced() {
    let Some(path) = corpus_file("cjk.pdf") else {
        return;
    };
    let text = full_text(&path);
    assert!(text.contains("參考文獻格式"), "CJK text missing");
    assert!(
        !text.contains("文 內 同 時"),
        "CJK glyphs were spaced apart"
    );
}
