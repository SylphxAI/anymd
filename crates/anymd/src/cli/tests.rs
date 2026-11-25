use super::{args::parse, mode, Mode};
use crate::lean::{read_text, ReadRender};
use crate::schema::ReadArgs;
use crate::source_access::SourceAccessPolicy;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn server_flags_and_subcommands_select_mcp() {
    assert!(matches!(mode(&args(&["mcp"])), Mode::Mcp));
    assert!(matches!(mode(&args(&["--allow-dir=/tmp"])), Mode::Mcp));
    assert!(matches!(mode(&args(&["doctor"])), Mode::Doctor));
    assert!(matches!(mode(&args(&["paper.pdf"])), Mode::Cli(_)));
}

#[test]
fn parses_named_ocr_without_consuming_a_filename() {
    use crate::ocr_vlm::{OcrEngine, OcrSelection};
    assert_eq!(
        parse(&args(&["--ocr", "vlm", "scan.png"])).unwrap().ocr,
        Some(OcrSelection::Engine(OcrEngine::Vlm))
    );
    let legacy = parse(&args(&["--ocr", "scan.png"])).unwrap();
    assert_eq!(legacy.inputs, ["scan.png"]);
    assert_eq!(legacy.ocr, Some(OcrSelection::Enabled(true)));
    assert_eq!(
        parse(&args(&["scan.png", "--ocr=tesseract"])).unwrap().ocr,
        Some(OcrSelection::Engine(OcrEngine::Tesseract))
    );
    assert!(parse(&args(&["--ocr=remote", "scan.png"])).is_err());
}

#[test]
fn parses_read_and_search_options() {
    let parsed = parse(&args(&[
        "a.pdf",
        "-p",
        "1-3",
        "--max-tokens=500",
        "--no-ocr",
    ]))
    .unwrap();
    assert_eq!(parsed.inputs, ["a.pdf"]);
    assert_eq!(parsed.pages.as_deref(), Some("1-3"));
    assert_eq!(parsed.max_tokens, Some(500));
    assert_eq!(
        parsed.ocr,
        Some(crate::ocr_vlm::OcrSelection::Enabled(false))
    );
    let parsed = parse(&args(&["attention", "docs", "--mode", "ranked", "-w"])).unwrap();
    assert_eq!(parsed.inputs, ["attention", "docs"]);
    assert_eq!(parsed.mode.as_deref(), Some("ranked"));
    assert!(parsed.whole_word);
    assert!(parse(&args(&["--bogus"])).is_err());
    let parsed = parse(&args(&["a.docx", "--images", "none"])).unwrap();
    assert_eq!(parsed.images.as_deref(), Some("none"));
    assert!(parse(&args(&["a.docx", "--images=all"])).is_err());
    let parsed = parse(&args(&["a.docx", "--revisions", "accept"])).unwrap();
    assert_eq!(parsed.revisions.as_deref(), Some("accept"));
    let parsed = parse(&args(&["a.docx", "--revisions=reject"])).unwrap();
    assert_eq!(parsed.revisions.as_deref(), Some("reject"));
    assert!(parse(&args(&["a.docx", "--revisions", "all"])).is_err());
    let parsed = parse(&args(&["talk.mp4", "--download-asr-model"])).unwrap();
    assert!(parsed.download_asr_model);
    let parsed = parse(&args(&["talk.mp4", "--download-whisper-model"])).unwrap();
    assert!(parsed.download_asr_model);
    assert_eq!(parsed.inputs, ["talk.mp4"]);
}

#[test]
fn download_presence_switches_reject_inline_values() {
    for flag in ["--download-asr-model", "--download-whisper-model"] {
        for value in ["false", "true", "invalid", "", "0", "1"] {
            assert!(parse(&args(&["talk.mp4", &format!("{flag}={value}")])).is_err());
        }
    }
    let parsed = parse(&args(&["talk.mp4", "--transcript"])).unwrap();
    assert!(!parsed.download_asr_model);
}

#[test]
fn converts_a_file_to_stdout_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.csv");
    std::fs::write(&path, "a,b\n1,2\n").unwrap();
    let (text, failed) = read_text(
        &ReadArgs {
            timeline: None,
            source: path.display().to_string(),
            node: None,
            pages: None,
            max_tokens: None,
            cursor: None,
            ocr: None,
            transcript: None,
            download_asr_model: None,
            images: None,
            revisions: None,
        },
        &SourceAccessPolicy::unrestricted(),
        &ReadRender {
            front_matter: false,
            unlimited: true,
        },
    )
    .unwrap();
    assert!(!failed);
    assert!(text.starts_with("|a|b|"), "{text}");
}
