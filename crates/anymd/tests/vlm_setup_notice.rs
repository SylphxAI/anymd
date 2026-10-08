//! VLM OCR without the one-time setup is guidance, not a failure. Its own test
//! binary because it points the cache at an empty directory.
#![cfg(not(feature = "ocr-vlm"))]

use anymd::schema::ReadArgs;
use anymd::source_access::SourceAccessPolicy;

const NOTICE: &str = "VLM OCR needs a one-time setup: run `anymd setup ocr` to download the local OCR engine and ~2 GB of model weights, then retry this read with OCR enabled. Setup opts into the download; documents stay on this machine.";

fn tiny_png(path: &std::path::Path) {
    image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255]))
        .save(path)
        .unwrap();
}

#[test]
fn vlm_request_without_setup_is_a_normal_tool_result() {
    let cache = tempfile::tempdir().unwrap();
    std::env::set_var("ANYMD_CACHE_DIR", cache.path());
    std::env::remove_var("ANYMD_OCR");
    let image = cache.path().join("scan.png");
    tiny_png(&image);
    let args: ReadArgs = serde_json::from_value(serde_json::json!({
        "source": image.to_string_lossy(),
        "ocr": "vlm",
    }))
    .unwrap();
    let policy = SourceAccessPolicy::default();

    let result = anymd::lean::read(&args, &policy).expect("a notice, not a protocol error");
    assert_ne!(result.is_error, Some(true));
    assert_eq!(anymd::lean::result_text(&result).trim_end(), NOTICE);

    // The CLI shows the same line and still exits non-zero.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_anymd"))
        .args(["--ocr", "vlm"])
        .arg(&image)
        .env("ANYMD_CACHE_DIR", cache.path())
        .env("ANYMD_NO_STAR_HINT", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(text.contains(NOTICE), "{text}");
    assert!(!text.contains("worker failed"), "{text}");

    // A first read of a scan also explains the download, without requiring
    // the caller to know which OCR engine to select. Hide system tesseract
    // so this readback is independent of the machine's optional tools.
    std::env::set_var("PATH", cache.path());
    let scan = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/scanned-page.pdf");
    let args: ReadArgs = serde_json::from_value(serde_json::json!({
        "source": scan.to_string_lossy(),
        "pages": "1",
    }))
    .unwrap();
    let result = anymd::lean::read(&args, &policy).expect("first-run scan guidance");
    assert_ne!(result.is_error, Some(true));
    let text = anymd::lean::result_text(&result);
    assert!(text.contains("scanned image"), "{text}");
    assert!(text.contains("~2 GB"), "{text}");
    assert!(text.contains("anymd setup ocr"), "{text}");
    assert!(text.contains("then retry"), "{text}");
    assert!(!text.contains("setup ocr / ocr: true"), "{text}");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_anymd"))
        .arg(&scan)
        .env("ANYMD_CACHE_DIR", cache.path())
        .env("ANYMD_NO_STAR_HINT", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("~2 GB"), "{text}");
    assert!(text.contains("then retry"), "{text}");
    assert!(!cache.path().join("models/docvlm-v1").exists());
}
