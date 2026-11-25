use super::provider::{provider_config_from, OcrOutputFormat, ProviderConfig, DEFAULT_TIMEOUT_MS};
use super::*;

#[test]
fn cite_check_ocr_uses_rotation_and_cropbox_inverse() {
    let mut output = json!({"words": [{"text": "word", "bounding_box": {"left":15., "bottom":70., "right":35., "top":100.}}]});
    restore_render_geometry(&mut output, [0., 2., 2., 0., -120., -100.], 2., 240);
    assert_eq!(
        output["words"][0]["bounding_box"],
        json!({"left":70., "bottom":75., "right":100., "top":95.})
    );
    restore_render_geometry(&mut output, [0.; 6], 2., 240);
    assert!(output["words"][0].get("bounding_box").is_none());
}

#[test]
fn vlm_evidence_keeps_order_boxes_and_layout_confidence_separate() {
    let output = normalize_output(
        r#"{"text":"cell","truncated":1,"words":[{"text":"cell","reading_order":2,"region_type":"table","layout_confidence":0.9,"bounding_box":{"left":10,"bottom":20,"right":30,"top":40}}]}"#,
        1000,
        &[],
        2.0,
        OcrOutputFormat::Auto,
        Some(100.0),
    );
    assert_eq!(output["words"][0]["reading_order"], 2);
    assert_eq!(output["words"][0]["bounding_box"]["left"], 5.0);
    assert_eq!(output["words"][0]["layout_confidence"], 0.9);
    assert!(output["words"][0].get("confidence").is_none());
    assert!(output["warnings"][0]
        .as_str()
        .unwrap()
        .contains("generation stopped"));
}

#[test]
fn worker_evidence_survives_typed_page_and_fusion_without_recognition_confidence() {
    let worker = json!({
        "text": "cell", "model_revision": "pinned-revision", "device": "metal", "quantization": "none",
        "words": [{"text": "cell", "reading_order": 2, "region_type": "table", "layout_confidence": 0.9,
            "bounding_box": {"left": 10, "bottom": 20, "right": 30, "top": 40}}]
    });
    let mut normalized = normalize_output(
        &worker.to_string(),
        1000,
        &[],
        2.0,
        OcrOutputFormat::Auto,
        Some(100.0),
    );
    let config = ProviderConfig {
        command: "anymd".into(),
        args_template: vec!["__ocr-vlm-worker".into()],
        output_format: OcrOutputFormat::Auto,
    };
    config.stamp_provenance(&mut normalized);
    normalized["page"] = json!(1);
    normalized["provider"] = json!("candle");
    normalized["source_render_evidence_id"] = json!("render-1");
    let page: OcrPage = serde_json::from_value(normalized).unwrap();
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
    let mut response = anymd_core::read_pdf::read_pdf_from_value(
        &json!({"sources": [{"path": fixture}], "auto": false}),
    )
    .unwrap();
    anymd_core::ocr_fusion::fuse_ocr_outcomes(
        &mut response,
        vec![SourceOcrOutcome {
            source_index: 0,
            pages: vec![page],
            warnings: Vec::new(),
            error: None,
        }],
    );
    let layer = response.results[0]
        .data
        .as_ref()
        .unwrap()
        .ocr_text_layer
        .as_ref()
        .unwrap();
    let fused = &layer["pages"][0];
    assert_eq!(fused["words"][0]["reading_order"], 2);
    assert_eq!(fused["words"][0]["region_type"], "table");
    assert_eq!(fused["words"][0]["layout_confidence"], 0.9);
    assert_eq!(fused["words"][0]["bounding_box"]["left"], 5.0);
    assert!(fused.get("confidence").is_none());
    assert!(fused["words"][0].get("confidence").is_none());
    assert_eq!(fused["provenance"]["model_revision"], "pinned-revision");
    assert_eq!(fused["provenance"]["device"], "metal");
    assert_eq!(fused["provenance"]["quantization"], "none");
    assert!(layer["summary"].get("average_confidence").is_none());
}

#[test]
fn provider_selection_controls_page_timeout_and_explicit_override() {
    let external = ProviderConfig {
        command: "external".into(),
        args_template: vec!["{input}".into()],
        output_format: OcrOutputFormat::Auto,
    };
    let builtin = ProviderConfig {
        command: "anymd".into(),
        args_template: vec!["__ocr-vlm-worker".into()],
        output_format: OcrOutputFormat::Auto,
    };
    assert_eq!(ReadOcrOptions::default().timeout_ms, None);
    assert_eq!(external.page_timeout(None).unwrap(), DEFAULT_TIMEOUT_MS);
    assert_eq!(
        builtin.page_timeout(None).unwrap(),
        crate::ocr_vlm::page_timeout().unwrap()
    );
    assert_eq!(builtin.page_timeout(Some(1234)).unwrap(), 1234);
    let deadline = Instant::now() - Duration::from_millis(1);
    assert!(crate::ocr_vlm::remaining_page_timeout(deadline).is_err());
    let deadline = Instant::now() + Duration::from_millis(1000);
    assert!(crate::ocr_vlm::remaining_page_timeout(deadline).unwrap() <= 1000);
}

#[test]
fn normalizes_json_provider_output_and_utf16_truncation() {
    let output = normalize_output(
        r#"{"text":"A😀B","confidence":87,"language":"fra","words":[{"text":"A","confidence":50,"bounding_box":{"left":2,"bottom":4,"right":6,"top":8}}]}"#,
        3,
        &[],
        2.0,
        OcrOutputFormat::Auto,
        None,
    );
    assert_eq!(output["text"], "A😀");
    assert_eq!(output["confidence"], 0.87);
    assert_eq!(output["language"], "fra");
    assert_eq!(output["words"][0]["confidence"], 0.5);
    assert_eq!(
        output["words"][0]["bounding_box"],
        json!({"left": 1.0, "bottom": 2.0, "right": 3.0, "top": 4.0})
    );
    assert_eq!(
        output["warnings"][0],
        "OCR output truncated to 3 characters."
    );
}

#[test]
fn rejects_invalid_provider_configuration_without_shell_parsing() {
    let error = provider_config_from(
        Some("provider".into()),
        None,
        Some(r#"["--no-input"]"#.into()),
    )
    .err()
    .expect("invalid args");
    assert!(error.contains("{input}"));
    let tsv = provider_config_from(None, Some("tesseract-tsv".into()), None)
        .expect("tsv preset is available");
    assert_eq!(tsv.command, "tesseract");
    assert_eq!(tsv.output_format, OcrOutputFormat::TesseractTsv);
    assert!(tsv.args_template.iter().any(|arg| arg == "tsv"));
}

#[test]
fn normalizes_tesseract_tsv_words_and_image_to_pdf_boxes() {
    let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t20\t10\t40\t20\t91\tHello\n5\t1\t1\t1\t1\t2\t70\t10\t30\t20\t88\tWorld\n";
    let output = normalize_output(
        tsv,
        200_000,
        &["eng".into()],
        2.0,
        OcrOutputFormat::TesseractTsv,
        Some(100.0),
    );
    assert_eq!(output["text"], "Hello World");
    assert_eq!(output["language"], "eng");
    assert_eq!(output["words"][0]["text"], "Hello");
    // image box left=20,top=10,width=40,height=20,imageHeight=100
    // bottom = 100-10-20=70, top=90; then /scale=2 => left=10,bottom=35,right=30,top=45
    assert_eq!(
        output["words"][0]["bounding_box"],
        json!({"left": 10.0, "bottom": 35.0, "right": 30.0, "top": 45.0})
    );
    assert_eq!(output["words"][0]["confidence"], 0.91);
}

#[test]
fn malformed_tesseract_tsv_returns_raw_with_warning() {
    let output = normalize_output(
        "not\ta\theader\nrow",
        200_000,
        &[],
        2.0,
        OcrOutputFormat::TesseractTsv,
        Some(100.0),
    );
    assert_eq!(output["text"], "not\ta\theader\nrow");
    assert_eq!(
        output["warnings"][0],
        "Tesseract TSV output could not be normalized; returned raw OCR output."
    );
    assert!(output.get("words").is_none());
}

#[test]
fn request_budget_is_request_wide_and_fail_closed() {
    let mut budget = RequestOcrBudget::default();
    budget
        .charge(
            MAX_REQUEST_PROVIDER_BYTES - 1,
            MAX_REQUEST_OUTPUT_CHARS - 1,
            refusal,
        )
        .expect("within budget");
    assert!(budget
        .charge(2, 0, refusal)
        .expect_err("byte overflow")
        .contains("bytes"));

    let mut budget = RequestOcrBudget::default();
    budget
        .charge(0, MAX_REQUEST_OUTPUT_CHARS - 1, refusal)
        .expect("within budget");
    assert!(budget
        .charge(0, 2, refusal)
        .expect_err("character overflow")
        .contains("characters"));
    assert!(budget
        .ensure_available()
        .expect_err("exhaustion is sticky")
        .contains("characters"));
}

#[test]
fn exhausted_provider_budget_blocks_later_render_and_provider_effects() {
    let mut request_budget = RequestOcrBudget::default();
    request_budget
        .charge(0, MAX_REQUEST_OUTPUT_CHARS + 1, refusal)
        .expect_err("exhaust provider budget");
    let mut render_budget = RequestWorkBudget::default();
    let mut sentinel_invocations = 0usize;

    if admit_read_ocr_source(
        &request_budget,
        &mut render_budget,
        Instant::now() + Duration::from_secs(1),
    )
    .is_ok()
    {
        sentinel_invocations += 1;
    }

    assert_eq!(sentinel_invocations, 0);
    assert_eq!(render_budget.rendered_pages, 0);
    assert_eq!(render_budget.render_pixels, 0);
}

#[test]
fn provider_concurrency_admission_is_process_wide_and_recoverable() {
    let mut first = crate::ocr_vlm::OcrRequest::acquire().expect("normal read permit");
    let deadline = first.run_page(|deadline, _| Ok(deadline)).unwrap();
    assert_eq!(
        first.run_page(|deadline, _| Ok(deadline)).unwrap(),
        deadline
    );
    assert!(first
        .run_page::<()>(|_, _| Err("first page failed".into()))
        .is_err());
    let mut later_workers = 0;
    assert!(first
        .run_page(|_, _| {
            later_workers += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(later_workers, 0);
    let second = acquire_permit().expect("second permit");
    assert!(acquire_permit()
        .expect_err("third request rejected")
        .contains("concurrency limit"));
    drop(first);
    let replacement = acquire_permit().expect("released slot is reusable");
    drop(second);
    drop(replacement);
    assert_eq!(crate::provider_budget::active_requests(), 0);
}
