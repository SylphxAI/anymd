use super::*;

fn raw_part(text: &str, x: f64, y: f64, width: f64) -> RawTextPart {
    let bounding_box = TextBoundingBox {
        left: x,
        bottom: y,
        right: x + width,
        top: y + 10.0,
    };
    let text_len = text.encode_utf16().count() as u32;
    let mut offset = 0u32;
    let chars = text
        .chars()
        .map(|character| {
            let value = character.to_string();
            let start = offset;
            offset += value.encode_utf16().count() as u32;
            TextCharacterGeometry {
                text: value,
                item_char_start: start,
                item_char_end: offset,
                is_whitespace: character.is_whitespace(),
                bounding_box: bounding_box.estimated_utf16_range(text_len, start, offset),
            }
        })
        .collect();
    let count = text.chars().count().max(1) as f64;
    let glyphs = (0..text.chars().count())
        .map(|index| {
            Some(GlyphExtent {
                x0: x + width * index as f64 / count,
                x1: x + width * (index + 1) as f64 / count,
                size: 10.0,
            })
        })
        .collect();
    RawTextPart {
        item: PositionedTextItem {
            text: text.to_string(),
            bounding_box: Some(bounding_box),
            chars,
            runs: Vec::new(),
        },
        glyphs,
        x: Some(x),
        y: Some(y),
        right: Some(x + width),
    }
}

#[test]
fn ltr_normalizer_matches_js_round_gap_and_stable_x_semantics() {
    let mut request_segments = 0;
    let items = normalize_page_text_parts(
        vec![
            raw_part("B", 60.0, 500.10, 10.0),
            raw_part("D", 0.0, 500.50, 10.0),
            raw_part("C", 118.01, 500.49, 10.0),
            raw_part("A", 2.0, 500.49, 10.0),
        ],
        &mut request_segments,
    )
    .expect("normalize LTR parts");

    assert_eq!(
        items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        // The 48-point gap joins the segment and is a word space.
        vec!["D", "A B", "C"]
    );
    assert_eq!(items[1].runs.len(), 2);
    assert_eq!(items[1].bounding_box.unwrap().left, 2.0);
    assert_eq!(items[1].bounding_box.unwrap().right, 70.0);
    assert_eq!(request_segments, 3);
}

#[test]
fn ltr_normalizer_uses_running_max_right_and_rebases_utf16_runs_and_chars() {
    let mut request_segments = 0;
    let items = normalize_page_text_parts(
        vec![
            raw_part("A😀", 0.0, 100.0, 100.0),
            raw_part("B", 20.0, 100.0, 5.0),
            raw_part("C", 148.0, 100.0, 5.0),
        ],
        &mut request_segments,
    )
    .expect("normalize overlapping parts");

    assert_eq!(items.len(), 1, "48-point gap from running max-right joins");
    assert_eq!(items[0].text, "A😀B C");
    assert_eq!(items[0].runs[0].item_char_end, 3);
    assert_eq!(items[0].runs[1].item_char_start, 3);
    assert_eq!(items[0].runs[1].text, "B ");
    assert_eq!(items[0].runs[2].item_char_start, 5);
    assert_eq!(items[0].chars[1].item_char_end, 3);
    assert_eq!(items[0].chars[2].item_char_start, 3);
    assert_eq!(items[0].chars[3].item_char_start, 4);
    assert!(items[0].chars[3].is_whitespace);
    assert_eq!(items[0].chars[4].item_char_start, 5);
}

/// A part whose glyphs sit at explicit starts with a fixed advance, like
/// one pdf-extract show-text string.
fn glyph_part(text: &str, starts: &[f64], advance: f64, size: f64, y: f64) -> RawTextPart {
    assert_eq!(text.chars().count(), starts.len());
    let mut part = raw_part(
        text,
        starts[0],
        y,
        starts.last().unwrap() + advance - starts[0],
    );
    part.glyphs = starts
        .iter()
        .map(|x0| {
            Some(GlyphExtent {
                x0: *x0,
                x1: x0 + advance,
                size,
            })
        })
        .collect();
    part
}

/// Glyph starts for `text` set solid (no tracking) from `x`.
fn solid(text: &str, x: f64, advance: f64) -> Vec<f64> {
    (0..text.chars().count())
        .map(|index| x + advance * index as f64)
        .collect()
}

fn merged(parts: Vec<RawTextPart>) -> PositionedTextItem {
    let mut request_segments = 0;
    let mut items =
        normalize_page_text_parts(parts, &mut request_segments).expect("normalize parts");
    assert_eq!(items.len(), 1);
    items.remove(0)
}

#[test]
fn tex_kerned_words_get_inferred_spaces_with_consistent_offsets() {
    // [(The)-333(dominan)28(t)-334(sequence)]TJ at 10pt: 3.33pt word gaps,
    // a 0.28pt kern inside "dominant".
    let adv = 5.0;
    let the = solid("The", 0.0, adv);
    let dominan = solid("dominan", 15.0 + 3.33, adv);
    let t_x = 18.33 + 35.0 - 0.28;
    let sequence = solid("sequence", t_x + adv + 3.34, adv);
    let item = merged(vec![
        glyph_part("The", &the, adv, 10.0, 700.0),
        glyph_part("dominan", &dominan, adv, 10.0, 700.0),
        glyph_part("t", &[t_x], adv, 10.0, 700.0),
        glyph_part("sequence", &sequence, adv, 10.0, 700.0),
    ]);
    assert_eq!(item.text, "The dominant sequence");
    assert_eq!(
        item.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        vec!["The ", "dominan", "t ", "sequence"]
    );
    let mut expected_start = 0;
    for run in &item.runs {
        assert_eq!(run.item_char_start, expected_start);
        assert_eq!(run.item_char_end - run.item_char_start, utf16(&run.text));
        expected_start = run.item_char_end;
    }
    assert_eq!(expected_start, utf16(&item.text));
    for character in &item.chars {
        let slice: String = item
            .text
            .encode_utf16()
            .skip(character.item_char_start as usize)
            .take((character.item_char_end - character.item_char_start) as usize)
            .map(|unit| char::from_u32(u32::from(unit)).unwrap())
            .collect();
        assert_eq!(slice, character.text);
    }
    let synthetic = &item.chars[3];
    assert!(synthetic.is_whitespace && synthetic.bounding_box.is_none());

    // The match box of a word after an inferred space is that word's box.
    let start = item.text.find("sequence").unwrap() as u32;
    let (box_, level) = match_bounding_box(&item, start, start + 8);
    let box_ = box_.expect("sequence box");
    assert_eq!(level.as_deref(), Some("char_estimated"));
    assert!((box_.left - sequence[0]).abs() < 1e-6);
    assert!((box_.right - (sequence[7] + adv)).abs() < 1e-6);
}

fn utf16(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

#[test]
fn inferred_spaces_split_words_inside_one_show_text_part() {
    // One TJ string whose glyph positions carry the word gap.
    let mut starts = solid("ab", 0.0, 5.0);
    starts.extend(solid("cd", 12.0, 5.0));
    let item = merged(vec![glyph_part("abcd", &starts, 5.0, 10.0, 300.0)]);
    assert_eq!(item.text, "ab cd");
    assert_eq!(item.runs.len(), 1);
    assert_eq!(item.runs[0].text, "ab cd");
    assert_eq!(item.runs[0].item_char_end, 5);
}

#[test]
fn explicit_spaces_are_not_doubled_and_tight_kerns_are_not_spaces() {
    let item = merged(vec![
        glyph_part("to ", &solid("to ", 0.0, 5.0), 5.0, 10.0, 300.0),
        glyph_part("be", &solid("be", 17.0, 5.0), 5.0, 10.0, 300.0),
        glyph_part("x", &[27.5], 5.0, 10.0, 300.0),
    ]);
    assert_eq!(item.text, "to bex");
}

#[test]
fn cjk_glyphs_stay_unspaced_unless_the_gap_is_wide() {
    let starts = [0.0, 10.5, 21.0, 31.5, 60.0];
    let item = merged(vec![glyph_part("中文排版字", &starts, 10.0, 10.0, 300.0)]);
    assert_eq!(item.text, "中文排版 字");
}

#[test]
fn letter_spaced_headings_keep_words_whole() {
    // "ABSTRACT NOW" tracked by 3pt per letter, 12pt word gap.
    let mut starts = Vec::new();
    let mut x = 0.0;
    for _ in 0..8 {
        starts.push(x);
        x += 7.0 + 3.0;
    }
    x += 9.0;
    for _ in 0..3 {
        starts.push(x);
        x += 7.0 + 3.0;
    }
    let item = merged(vec![glyph_part("ABSTRACTNOW", &starts, 7.0, 10.0, 300.0)]);
    assert_eq!(item.text, "ABSTRACT NOW");
}

#[test]
fn small_superscript_after_a_word_is_not_spaced() {
    // "x" at 10pt followed by a 7pt "2" set tight in the same row.
    let item = merged(vec![
        glyph_part("x", &[0.0], 5.0, 10.0, 300.0),
        glyph_part("2", &[5.3], 3.5, 7.0, 300.2),
    ]);
    assert_eq!(item.text, "x2");
}

#[test]
fn unknown_glyph_geometry_never_guesses_a_space() {
    let mut part = glyph_part("ab", &[0.0, 40.0], 5.0, 10.0, 300.0);
    part.glyphs[1] = None;
    assert_eq!(merged(vec![part]).text, "ab");
}

#[test]
fn raw_part_admission_accepts_exact_caps_and_rejects_cap_plus_one() {
    let mut output = TextItemOutput::default();
    for _ in 0..MAX_RAW_TEXT_PARTS_PER_PAGE {
        output.admit_part().expect("exact page cap");
    }
    assert!(output.admit_part().is_err(), "page cap + 1 must fail");

    let mut output = TextItemOutput::default();
    for index in 0..MAX_RAW_TEXT_PARTS {
        if index % MAX_RAW_TEXT_PARTS_PER_PAGE == 0 {
            output.page_raw_part_count = 0;
        }
        output.admit_part().expect("exact request cap");
    }
    output.page_raw_part_count = 0;
    assert!(output.admit_part().is_err(), "request cap + 1 must fail");

    let mut output = TextItemOutput {
        raw_part_count: MAX_RAW_TEXT_PARTS,
        ..TextItemOutput::default()
    };
    assert!(output.begin_word().is_err());
    assert!(
        output.current_part.is_none(),
        "cap rejection must precede raw-part allocation"
    );
}

#[test]
fn normalized_segment_admission_is_exact_and_never_returns_partial_output() {
    let exact = (0..MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE)
        .map(|index| raw_part("x", index as f64 * 60.0, 100.0, 1.0))
        .collect();
    let mut request_segments = 0;
    assert_eq!(
        normalize_page_text_parts(exact, &mut request_segments)
            .expect("exact normalized page cap")
            .len(),
        MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE
    );

    let over = (0..=MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE)
        .map(|index| raw_part("x", index as f64 * 60.0, 100.0, 1.0))
        .collect();
    let mut request_segments = 0;
    assert!(normalize_page_text_parts(over, &mut request_segments).is_err());

    let mut request_segments = MAX_NORMALIZED_TEXT_SEGMENTS - 1;
    normalize_page_text_parts(vec![raw_part("x", 0.0, 0.0, 1.0)], &mut request_segments)
        .expect("exact normalized request cap");
    assert_eq!(request_segments, MAX_NORMALIZED_TEXT_SEGMENTS);
    assert!(
        normalize_page_text_parts(vec![raw_part("x", 0.0, 0.0, 1.0)], &mut request_segments)
            .is_err()
    );
}

fn two_blank_page_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Resources << >> /Contents 5 0 R >>"
            .to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Resources << >> /Contents 6 0 R >>"
            .to_string(),
        "<< /Length 0 >>\nstream\nendstream".to_string(),
        "<< /Length 0 >>\nstream\nendstream".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

fn positioned_text_pdf(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 1 0 0 1 72 700 Tm ({text}) Tj ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

fn multi_page_text_pdf(page_texts: &[String]) -> Vec<u8> {
    let page_count = page_texts.len();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            (3..3 + page_count)
                .map(|index| format!("{index} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
            page_count
        ),
    ];
    let mut contents_object = 3 + page_count;
    let font_object = contents_object + page_count;
    for _text in page_texts {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font_object} 0 R >> >> /Contents {contents_object} 0 R >>"
        ));
        contents_object += 1;
    }
    for text in page_texts {
        let content = format!("BT /F1 12 Tf 1 0 0 1 72 700 Tm ({text}) Tj ET");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[test]
fn per_page_budget_reset_accepts_dense_multi_page_documents() {
    // Two pages whose cumulative text exceeds the old request-wide
    // geometry budget, while each page stays inside the per-page cap.
    let page_texts = vec!["x".repeat(130_000), "y".repeat(130_000)];
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("dense-two-pages.pdf");
    std::fs::write(&path, multi_page_text_pdf(&page_texts)).expect("write PDF");
    let pages = extract_page_texts(&path, 4_000_000).expect("dense pages must extract");
    assert_eq!(pages, page_texts);
}

#[test]
fn single_page_over_per_page_budget_still_fails_closed() {
    let page_texts = vec!["x".repeat(MAX_GEOMETRY_CHARS + 1)];
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("over-dense-page.pdf");
    std::fs::write(&path, multi_page_text_pdf(&page_texts)).expect("write PDF");
    let error = extract_page_texts(&path, 4_000_000).expect_err("per-page cap must hold");
    assert!(
        error.message.contains("bounded extraction budget"),
        "unexpected error: {:?}",
        error
    );
}

#[test]
fn document_backstop_still_fails_closed() {
    let mut output = TextItemOutput {
        total_geometry_chars: MAX_TOTAL_GEOMETRY_CHARS,
        total_text_bytes: MAX_TOTAL_EXTRACTED_TEXT_BYTES,
        pages: vec![Vec::new()],
        ..TextItemOutput::default()
    };
    output.begin_word().expect("begin word");
    let error = output
        .output_character(&Transform::identity(), 1.0, 0.0, 12.0, "x")
        .expect_err("document backstop must fail closed");
    assert!(
        error
            .to_string()
            .contains("bounded document extraction budget"),
        "unexpected error: {error}"
    );
}

#[test]
fn page_counters_reset_and_document_counters_accumulate() {
    let mut output = TextItemOutput {
        text_bytes: MAX_EXTRACTED_TEXT_BYTES,
        geometry_chars: MAX_GEOMETRY_CHARS,
        total_text_bytes: 3,
        total_geometry_chars: 3,
        pages: vec![Vec::new()],
        ..TextItemOutput::default()
    };
    output.begin_word().expect("begin word");
    assert!(
        output
            .output_character(&Transform::identity(), 1.0, 0.0, 12.0, "x")
            .is_err(),
        "exhausted per-page budget must fail before begin_page resets it"
    );
    output
        .begin_page(
            2,
            &MediaBox {
                llx: 0.0,
                lly: 0.0,
                urx: 612.0,
                ury: 792.0,
            },
            None,
        )
        .expect("begin page");
    output.begin_word().expect("begin word");
    output
        .output_character(&Transform::identity(), 1.0, 0.0, 12.0, "ab")
        .expect("fresh per-page budget after begin_page");
    // Note: the rejected character above still incremented the cumulative
    // totals (admission happens before the cap check), so they start at 4.
    assert_eq!(output.text_bytes, 2);
    assert_eq!(output.geometry_chars, 1);
    assert_eq!(output.total_text_bytes, 6);
    assert_eq!(output.total_geometry_chars, 5);
}

#[test]
fn preserves_real_pdf_page_boundaries_including_blank_pages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("two-pages.pdf");
    std::fs::write(&path, two_blank_page_pdf()).expect("write PDF");
    let pages = extract_page_texts(&path, 1_000_000).expect("extract pages");
    assert_eq!(pages, vec![String::new(), String::new()]);
}

#[test]
fn captures_bounded_selectable_text_geometry_and_search_match_boxes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("positioned.pdf");
    std::fs::write(&path, positioned_text_pdf("Alpha beta")).expect("write PDF");

    let extracted = extract_pdf_text(&path, 1_000_000).expect("extract geometry");
    let item = &extracted.pages[0].positioned_items[0];
    assert_eq!(item.text, "Alpha beta");
    assert_eq!(item.chars.len(), 10);
    assert_eq!(item.chars[0].item_char_start, 0);
    assert_eq!(item.chars[0].item_char_end, 1);
    assert!(item.bounding_box.is_some());
    assert!(item
        .chars
        .iter()
        .all(|character| character.bounding_box.is_some()));

    let result =
        search_pdf_text(&path, 1_000_000, "beta", true, false, 1, 5, 10).expect("search geometry");
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].match_start, 6);
    assert_eq!(result.matches[0].match_end, 10);
    assert_eq!(
        result.matches[0].bounding_box_level.as_deref(),
        Some("char_estimated")
    );
    let match_box = result.matches[0].bounding_box.expect("match box");
    let item_box = item.bounding_box.expect("item box");
    assert!(match_box.left > item_box.left);
    assert_eq!(match_box.right, item_box.right);
}

#[test]
fn captures_geometry_for_behavior_fixture() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-behavior-v1.pdf");
    let extracted = extract_pdf_text(&path, 1_000_000).expect("extract fixture");
    let needle_items = extracted
        .pages
        .iter()
        .flat_map(|page| &page.positioned_items)
        .filter(|item| item.text.to_lowercase().contains("needle"))
        .collect::<Vec<_>>();
    assert!(
        needle_items.iter().all(|item| item.bounding_box.is_some()),
        "needle items: {needle_items:?}"
    );
}

#[test]
fn geometry_is_finite_utf16_aware_and_bounded() {
    let item_box = TextBoundingBox {
        left: 10.0,
        bottom: 20.0,
        right: 50.0,
        top: 30.0,
    };
    // A😀B has four JavaScript UTF-16 code units; the astral character owns two.
    let astral = item_box
        .estimated_utf16_range(4, 1, 3)
        .expect("astral range box");
    assert_eq!(astral.left, 20.0);
    assert_eq!(astral.right, 40.0);

    let nonfinite = Transform::row_major(1.0, 0.0, 0.0, 1.0, f64::NAN, 20.0);
    assert!(TextBoundingBox::from_character(&nonfinite, 1.0, 12.0).is_none());

    let mut output = TextItemOutput {
        text_bytes: MAX_EXTRACTED_TEXT_BYTES,
        ..TextItemOutput::default()
    };
    output.begin_word().expect("begin word");
    let error = output
        .output_character(&Transform::identity(), 1.0, 0.0, 12.0, "x")
        .expect_err("budget must fail closed");
    assert!(error.to_string().contains("bounded extraction budget"));
}

#[test]
fn source_coordinates_canonicalize_f32_expansion_at_four_decimal_places() {
    assert_eq!(canonical_coordinate(699.499_023_438), Some(699.499));
    assert_eq!(canonical_coordinate(123.000_999_451), Some(123.001));
    assert_eq!(canonical_coordinate(701.000_000_047), Some(701.0));
    assert_eq!(canonical_coordinate(-0.000_001), Some(0.0));
    assert!(!canonical_coordinate(-0.000_001)
        .expect("canonical zero")
        .is_sign_negative());
    assert_eq!(canonical_coordinate(f64::NAN), None);
    assert_eq!(canonical_coordinate(f64::INFINITY), None);
    assert_eq!(canonical_coordinate(f64::MAX), None);

    let transform = Transform::row_major(1.0, 0.0, 0.0, 1.0, 123.000_999_451, 699.499_023_438);
    let box_ = TextBoundingBox::from_character(&transform, 1.0, 1.500_976_609)
        .expect("canonical character box");
    assert_eq!(box_.left, 123.001);
    assert_eq!(box_.bottom, 699.499);
    assert_eq!(box_.right, 124.502);
    assert_eq!(box_.top, 701.0);
}

#[test]
fn opposite_extreme_finite_coordinates_fail_item_geometry_closed() {
    let left = TextBoundingBox {
        left: -1.0e308,
        bottom: 0.0,
        right: -9.0e307,
        top: 12.0,
    };
    let right = TextBoundingBox {
        left: 9.0e307,
        bottom: 0.0,
        right: 1.0e308,
        top: 12.0,
    };
    assert!(left.union(right).is_none());
    assert!(TextBoundingBox {
        left: -1.0e308,
        bottom: 0.0,
        right: 1.0e308,
        top: 12.0,
    }
    .estimated_utf16_range(2, 0, 1)
    .is_none());

    let mut output = TextItemOutput {
        pages: vec![Vec::new()],
        ..TextItemOutput::default()
    };
    output.begin_word().expect("begin word");
    let left_transform = Transform::row_major(1.0, 0.0, 0.0, 1.0, -1.0e308, 10.0);
    let right_transform = Transform::row_major(1.0, 0.0, 0.0, 1.0, 1.0e308, 10.0);
    output
        .output_character(&left_transform, 1.0, 0.0, 12.0, "a")
        .expect("left character");
    output
        .output_character(&right_transform, 1.0, 0.0, 12.0, "b")
        .expect("right character");
    output.end_line().expect("end line");
    let item = &output.pages[0][0];
    assert_eq!(item.item.bounding_box, None);
    assert!(item
        .item
        .chars
        .iter()
        .all(|character| character.bounding_box.is_none()));
}

#[test]
fn individual_invalid_character_geometry_is_sticky_in_both_orders() {
    let valid = Transform::row_major(1.0, 0.0, 0.0, 1.0, 72.0, 700.0);
    let invalid = Transform::row_major(1.0, 0.0, 0.0, 1.0, f64::NAN, 700.0);
    for transforms in [[&invalid, &valid], [&valid, &invalid]] {
        let mut output = TextItemOutput {
            pages: vec![Vec::new()],
            ..TextItemOutput::default()
        };
        output.begin_word().expect("begin word");
        for (index, transform) in transforms.into_iter().enumerate() {
            output
                .output_character(
                    transform,
                    1.0,
                    0.0,
                    12.0,
                    if index == 0 { "a" } else { "b" },
                )
                .expect("character callback remains recoverable");
        }
        output.end_line().expect("end line");
        let item = &output.pages[0][0];
        assert_eq!(item.item.bounding_box, None);
        assert!(item
            .item
            .chars
            .iter()
            .all(|character| character.bounding_box.is_none()));
    }
}

#[test]
fn whitespace_only_match_falls_back_to_text_item_box() {
    let item_box = TextBoundingBox {
        left: 10.0,
        bottom: 20.0,
        right: 50.0,
        top: 30.0,
    };
    let item = PositionedTextItem {
        text: " ".into(),
        bounding_box: Some(item_box),
        chars: vec![TextCharacterGeometry {
            text: " ".into(),
            item_char_start: 0,
            item_char_end: 1,
            is_whitespace: true,
            bounding_box: Some(item_box),
        }],
        runs: Vec::new(),
    };
    let (box_, level) = match_bounding_box(&item, 0, 1);
    assert_eq!(box_, Some(item_box));
    assert_eq!(level.as_deref(), Some("text_item"));
}

#[test]
fn geometry_index_bounds_exact_cap_match_queries() {
    let box_ = TextBoundingBox {
        left: 0.0,
        bottom: 0.0,
        right: 1.0,
        top: 1.0,
    };
    let mut chars = Vec::with_capacity(MAX_GEOMETRY_CHARS);
    for offset in 0..MAX_GEOMETRY_CHARS as u32 {
        chars.push(TextCharacterGeometry {
            text: String::new(),
            item_char_start: offset,
            item_char_end: offset + 1,
            is_whitespace: false,
            bounding_box: Some(box_),
        });
    }
    let item = PositionedTextItem {
        text: String::new(),
        bounding_box: None,
        chars,
        runs: Vec::new(),
    };
    let index = TextGeometryIndex::new(&item);
    for offset in 0..500u32 {
        assert_eq!(
            index.match_bounding_box(offset, offset + 1),
            (Some(box_), Some("char_estimated".to_string()))
        );
    }
    assert_eq!(
        index.candidate_visits(),
        1_000,
        "500 production matcher calls must range-query the index instead of rescanning all {MAX_GEOMETRY_CHARS} chars"
    );
}

fn reference_match_bounding_box(
    item: &PositionedTextItem,
    start_utf16: u32,
    end_utf16: u32,
) -> (Option<TextBoundingBox>, Option<String>) {
    let char_box = item
        .chars
        .iter()
        .filter(|character| {
            !character.is_whitespace
                && character.item_char_end >= character.item_char_start
                && character.item_char_start >= start_utf16
                && character.item_char_end <= end_utf16
        })
        .filter_map(|character| character.bounding_box)
        .try_fold(None::<TextBoundingBox>, |current, box_| match current {
            None => Some(Some(box_)),
            Some(current) => current.union(box_).map(Some),
        })
        .flatten();
    if let Some(box_) = char_box {
        (Some(box_), Some("char_estimated".to_string()))
    } else if let Some(box_) = item.bounding_box {
        (Some(box_), Some("text_item".to_string()))
    } else {
        (None, None)
    }
}

#[test]
fn geometry_index_matches_reference_for_irregular_internal_ranges() {
    let item_box = TextBoundingBox {
        left: -10.0,
        bottom: -10.0,
        right: 10.0,
        top: 10.0,
    };
    let boxes = [
        TextBoundingBox {
            left: 2.0,
            bottom: 0.0,
            right: 3.0,
            top: 1.0,
        },
        TextBoundingBox {
            left: 0.0,
            bottom: 0.0,
            right: 1.0,
            top: 1.0,
        },
        TextBoundingBox {
            left: 1.0,
            bottom: 0.0,
            right: 2.0,
            top: 1.0,
        },
    ];
    let item = PositionedTextItem {
        text: "abc".to_string(),
        bounding_box: Some(item_box),
        chars: vec![
            TextCharacterGeometry {
                text: "c".to_string(),
                item_char_start: 2,
                item_char_end: 3,
                is_whitespace: false,
                bounding_box: Some(boxes[0]),
            },
            TextCharacterGeometry {
                text: "".to_string(),
                item_char_start: 1,
                item_char_end: 1,
                is_whitespace: false,
                bounding_box: Some(boxes[1]),
            },
            TextCharacterGeometry {
                text: "malformed".to_string(),
                item_char_start: 2,
                item_char_end: 1,
                is_whitespace: false,
                bounding_box: Some(boxes[2]),
            },
            TextCharacterGeometry {
                text: "b".to_string(),
                item_char_start: 1,
                item_char_end: 2,
                is_whitespace: false,
                bounding_box: Some(boxes[1]),
            },
            TextCharacterGeometry {
                text: "ignored".to_string(),
                item_char_start: 0,
                item_char_end: 3,
                is_whitespace: true,
                bounding_box: Some(boxes[0]),
            },
            TextCharacterGeometry {
                text: "unboxed".to_string(),
                item_char_start: 0,
                item_char_end: 1,
                is_whitespace: false,
                bounding_box: None,
            },
        ],
        runs: Vec::new(),
    };
    let index = TextGeometryIndex::new(&item);
    for (start, end) in [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 3), (3, 3)] {
        assert_eq!(
            index.match_bounding_box(start, end),
            reference_match_bounding_box(&item, start, end),
            "range {start}..{end}"
        );
    }
}

#[test]
fn finds_literal_matches_with_snippets() {
    let text = "Sample PDF file with sample terms inside.";
    let matches = find_matches_in_text(text, "sample", false, false).unwrap();
    assert_eq!(matches.len(), 2);
    let projection = SourceUtf16Projection::new(text);
    let snippet = build_snippet(
        text,
        &projection,
        matches[0].start_utf16,
        matches[0].end_utf16,
        6,
    )
    .unwrap();
    assert!(snippet.contains("Sample PDF"));
}

#[test]
fn respects_whole_word_boundaries() {
    let text = "risk risky risk";
    let matches = find_matches_in_text(text, "risk", false, true).unwrap();
    assert_eq!(matches.len(), 2);
}

#[test]
fn does_not_panic_on_multibyte_ligature_when_searching_ascii() {
    // ﬁ lowercases to "fi" (2 chars); naive byte-index search used to panic in snippets.
    let text = "preﬁx and more text around here for context";
    let matches = find_matches_in_text(text, "a", false, false).unwrap();
    assert!(!matches.is_empty());
    let projection = SourceUtf16Projection::new(text);
    for item_match in matches {
        let _ = build_snippet(
            text,
            &projection,
            item_match.start_utf16,
            item_match.end_utf16,
            40,
        )
        .unwrap();
        assert!(text.is_char_boundary(item_match.source_start));
        assert!(text.is_char_boundary(item_match.source_end));
    }
}

#[test]
fn reports_offsets_in_javascript_utf16_code_units() {
    let text = "😀Café";
    let matches = find_matches_in_text(text, "Café", true, false).unwrap();
    assert_eq!(matches[0].start_utf16, 2);
    assert_eq!(matches[0].end_utf16, 6);
}

#[test]
fn projects_length_changing_lowercase_indices_like_v3_0_14() {
    let text = "A İX Z ASCII ";

    let capital_i_dot = find_matches_in_text(text, "İ", false, false).unwrap()[0];
    assert_eq!((capital_i_dot.start_utf16, capital_i_dot.end_utf16), (2, 4));
    assert_eq!(
        &text[capital_i_dot.source_start..capital_i_dot.source_end],
        "İX"
    );

    let combining_dot = find_matches_in_text(text, "\u{307}", false, false).unwrap()[0];
    assert_eq!((combining_dot.start_utf16, combining_dot.end_utf16), (3, 4));
    assert_eq!(
        &text[combining_dot.source_start..combining_dot.source_end],
        "X"
    );

    let ascii = find_matches_in_text(text, "ASCII", false, true).unwrap()[0];
    assert_eq!((ascii.start_utf16, ascii.end_utf16), (8, 13));
    assert_eq!(&text[ascii.source_start..ascii.source_end], "SCII ");
}

#[test]
fn lowercase_index_projection_fails_closed_at_split_surrogate() {
    let error = find_matches_in_text("İ😀", "😀", false, false).unwrap_err();
    assert_eq!(error.code, TextIndexErrorCode::InvalidRequest);
    assert!(error.message.contains("split an astral character"));
}

#[test]
fn lowercase_index_projection_preserves_clamping_nonoverlap_and_normalized_words() {
    let clamped_text = "Aİ";
    let clamped = find_matches_in_text(clamped_text, "\u{307}", false, false).unwrap()[0];
    assert_eq!((clamped.start_utf16, clamped.end_utf16), (2, 3));
    assert_eq!(clamped.source_start, clamped_text.len());
    assert_eq!(clamped.source_end, clamped_text.len());
    let projection = SourceUtf16Projection::new(clamped_text);
    assert_eq!(
        build_snippet(
            clamped_text,
            &projection,
            clamped.start_utf16,
            clamped.end_utf16,
            0,
        )
        .unwrap(),
        "..."
    );

    let nonoverlapping = find_matches_in_text("aaaa", "aa", false, false).unwrap();
    assert_eq!(
        nonoverlapping
            .iter()
            .map(|range| (range.start_utf16, range.end_utf16))
            .collect::<Vec<_>>(),
        vec![(0, 2), (2, 4)]
    );

    // The combining dot before `A` exists only in normalized text. TS
    // therefore admits this whole-word match even though direct projection
    // to the original string would inspect a different neighbour.
    let normalized_word = find_matches_in_text("İA", "A", false, true).unwrap()[0];
    assert_eq!(
        (normalized_word.start_utf16, normalized_word.end_utf16),
        (2, 3)
    );
    assert_eq!(normalized_word.source_start, "İA".len());
    assert_eq!(normalized_word.source_end, "İA".len());

    // Conversely, the normalized `A` immediately before this space is a
    // word character, so TS rejects it even though the same projected
    // original boundary has a space as its preceding character.
    assert!(find_matches_in_text("İA ", " ", false, true)
        .unwrap()
        .is_empty());
}

#[test]
fn snippet_context_counts_utf16_units_like_v3_0_14() {
    let text = "Café résumé";
    let item_match = find_matches_in_text(text, "résumé", true, false).unwrap()[0];
    let projection = SourceUtf16Projection::new(text);
    assert_eq!(
        build_snippet(
            text,
            &projection,
            item_match.start_utf16,
            item_match.end_utf16,
            5,
        )
        .unwrap(),
        "Café résumé"
    );
    assert_eq!(
        build_snippet(
            text,
            &projection,
            item_match.start_utf16,
            item_match.end_utf16,
            0,
        )
        .unwrap(),
        "...résumé"
    );

    // Rust strings cannot contain lone UTF-16 surrogates. Fail closed when
    // the exact TS context boundary would bisect an astral scalar.
    let astral = "A😀résuméZ";
    let item_match = find_matches_in_text(astral, "résumé", true, false).unwrap()[0];
    let projection = SourceUtf16Projection::new(astral);
    let error = build_snippet(
        astral,
        &projection,
        item_match.start_utf16,
        item_match.end_utf16,
        1,
    )
    .unwrap_err();
    assert_eq!(error.code, TextIndexErrorCode::InvalidRequest);
    assert!(error.message.contains("split an astral character"));
    assert_eq!(
        build_snippet(
            astral,
            &projection,
            item_match.start_utf16,
            item_match.end_utf16,
            2,
        )
        .unwrap(),
        "...😀résuméZ"
    );

    let trailing_astral = "résumé😀A";
    let item_match = find_matches_in_text(trailing_astral, "résumé", true, false).unwrap()[0];
    let projection = SourceUtf16Projection::new(trailing_astral);
    let error = build_snippet(
        trailing_astral,
        &projection,
        item_match.start_utf16,
        item_match.end_utf16,
        1,
    )
    .unwrap_err();
    assert_eq!(error.code, TextIndexErrorCode::InvalidRequest);
    assert_eq!(
        build_snippet(
            trailing_astral,
            &projection,
            item_match.start_utf16,
            item_match.end_utf16,
            2,
        )
        .unwrap(),
        "résumé😀..."
    );
}

#[test]
fn search_does_not_persist_extracted_text_beside_source() {
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/sample.pdf");
    if !fixture.is_file() {
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let pdf_path = temp.path().join("sample.pdf");
    std::fs::copy(&fixture, &pdf_path).expect("copy fixture");

    let first = search_pdf_text(
        pdf_path.as_path(),
        1024 * 1024,
        "Lorem",
        false,
        false,
        10,
        10,
        32,
    )
    .expect("first search");
    assert_eq!(first.page_cache, None);
    assert!(first.total_matches > 0);

    let second = search_pdf_text(
        pdf_path.as_path(),
        1024 * 1024,
        "Lorem",
        false,
        false,
        10,
        10,
        32,
    )
    .expect("second search");
    assert_eq!(second.page_cache, None);
    assert_eq!(second.total_matches, first.total_matches);
    assert!(!temp.path().join(".pdf-reader-mcp").exists());
}

#[test]
fn bulk_normalize_case_and_empty_query() {
    assert_eq!(
        find_matches_in_text("AbC", "abc", false, false)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        find_matches_in_text("AbC", "AbC", true, false)
            .unwrap()
            .len(),
        1
    );
    assert!(find_matches_in_text("AbC", "abc", true, false)
        .unwrap()
        .is_empty());
    assert!(find_matches_in_text("hello", "", false, false)
        .unwrap()
        .is_empty());
    assert!(find_matches_in_text("hello", "z", false, false)
        .unwrap()
        .is_empty());
}

#[test]
fn bulk_whole_word_and_snippet_ellipsis() {
    let text = "alpha beta alphabet";
    let whole = find_matches_in_text(text, "alpha", false, true).unwrap();
    assert_eq!(whole.len(), 1, "{whole:?}");
    let loose = find_matches_in_text(text, "alpha", false, false).unwrap();
    assert!(loose.len() >= 2, "{loose:?}");
    assert!(is_word_char(Some('a')));
    assert!(is_word_char(Some('_')));
    assert!(!is_word_char(Some(' ')));
    assert!(!is_word_char(None));
    let snippet_text = "0123456789abcdefghij";
    let projection = SourceUtf16Projection::new(snippet_text);
    let snip = build_snippet(snippet_text, &projection, 8, 12, 2).unwrap();
    assert!(snip.contains("..."), "{snip}");
    let short = "short";
    let short_projection = SourceUtf16Projection::new(short);
    let snip2 = build_snippet(short, &short_projection, 0, 5, 10).unwrap();
    assert_eq!(snip2, "short");
}

#[test]
fn extraction_contains_cff_custom_encoding_charset_panic() {
    // Regression for SylphxAI/pdf-reader-mcp#660. pdf-extract unwraps
    // cff-parser while loading a Type1C font. A malformed Custom encoding
    // that is longer than its charset used to panic at encoding.rs and
    // escape as a server/task abort instead of a structured MCP error.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../test/fixtures/differential/v3014-bug660-cff-custom-encoding-short-charset-v1.pdf",
    );
    let error = extract_pdf_text(&fixture, 256 * 1024 * 1024)
        .expect_err("CFF Custom encoding panic must be contained");
    assert_eq!(error.code, TextIndexErrorCode::ExtractionFailed);
    assert!(error.message.contains("malformed font encoding"));
}

fn inline_image_pdf(content: &[u8]) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << >> >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            String::from_utf8_lossy(content)
        ),
    ];
    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

fn write_inline_pdf(content: &[u8]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("inline.pdf"), inline_image_pdf(content)).expect("write PDF");
    temp
}

#[test]
fn extraction_survives_wellformed_inline_images() {
    // Regression for SylphxAI/pdf-reader-mcp#675: the issue's reproducer
    // (`BI /W 1 /H 1 /IM true /BPC 1 ID <0x00> EI`) and its variant table
    // (grey colorspace, full key names, 8x8 data, `q..Q` wrapping, 0xFF
    // data byte, `EI`-like bytes in data) must extract without panicking.
    let variants: &[&[u8]] = &[
        b"BI /W 1 /H 1 /IM true /BPC 1 ID \x00 EI\n",
        b"BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI\n",
        b"BI /W 8 /H 8 /IM true /BPC 1 ID \x00\x00\x00\x00\x00\x00\x00\x00 EI\n",
        b"BI /Width 1 /Height 1 /ImageMask true /BitsPerComponent 1 ID \x00 EI\n",
        b"q BI /W 1 /H 1 /IM true /BPC 1 ID \x00 EI Q\n",
        b"BI /W 1 /H 1 /IM true /BPC 1 ID \xFF EI\n",
        b"BI /W 16 /H 1 /IM true /BPC 1 ID EI EI\n",
        b"BI /W 1 /H 1 /CS /DeviceGray /BPC 8 ID \x00 EI\n",
    ];
    for (index, content) in variants.iter().enumerate() {
        let temp = write_inline_pdf(content);
        let extracted = extract_pdf_text(&temp.path().join("inline.pdf"), 256 * 1024 * 1024)
            .unwrap_or_else(|err| panic!("variant {index} must extract: {}", err.message));
        assert_eq!(extracted.pages.len(), 1, "variant {index}");
    }
}

#[test]
fn extraction_reports_malformed_inline_image_as_page_error() {
    // Regression for SylphxAI/pdf-reader-mcp#675 request 1: a malformed
    // content stream must surface as a page-level tool error naming the
    // page — never a panicked worker. A missing `/CS` (without `/IM`)
    // panics inside lopdf 0.42's inline-image parser (`unwrap()` of
    // `DictKey("ColorSpace")`); it must arrive here as
    // `invalid content stream (page 1)`.
    let malformed: &[u8] = b"BI /W 1 /H 1 /BPC 1 ID \x00 EI\n";
    let temp = write_inline_pdf(malformed);
    let error = extract_pdf_text(&temp.path().join("inline.pdf"), 256 * 1024 * 1024)
        .expect_err("malformed inline image must be a structured error");
    assert_eq!(error.code, TextIndexErrorCode::ExtractionFailed);
    assert!(
        error.message.contains("invalid content stream (page 1)"),
        "unexpected message: {}",
        error.message
    );
}

#[test]
fn extraction_rejects_truncated_inline_image_data() {
    // An 8x8 1-bit mask needs 8 data bytes; 2 bytes must fail as a
    // page-level error rather than a literal-`EI` scan or a panic.
    let truncated: &[u8] = b"BI /W 8 /H 8 /IM true /BPC 1 ID \x00\x00 EI\n";
    let temp = write_inline_pdf(truncated);
    let error = extract_pdf_text(&temp.path().join("inline.pdf"), 256 * 1024 * 1024)
        .expect_err("truncated inline image must be a structured error");
    assert_eq!(error.code, TextIndexErrorCode::ExtractionFailed);
    assert!(
        error.message.contains("invalid content stream (page 1)"),
        "unexpected message: {}",
        error.message
    );
}

#[test]
fn extraction_survives_cid_cmap_with_odd_length_bfrange_destination() {
    // Regression for SylphxAI/pdf-reader-mcp#608. pdfTeX files ship ToUnicode
    // CMaps with 1-byte beginbfrange destinations (e.g. <C5> <D6> <C5>), which
    // made the upstream adobe-cmap-parser panic with "bad length of hexstring".
    // pdf-extract unwraps that Result, so the panic used to abort the whole
    // pdf-reader-mcp process. This fixture reproduces the exact crashing
    // construct and must extract without panicking.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-bug608-cid-bfrange-odd-v1.pdf");
    let extracted = extract_pdf_text(&fixture, 256 * 1024 * 1024)
        .expect("malformed CMap must not abort extraction");
    assert_eq!(extracted.pages.len(), 1);
    assert_eq!(extracted.info.format_version, "1.4");

    // The search path is the same one exposed over MCP; it must complete.
    let search = search_pdf_text(&fixture, 256 * 1024 * 1024, "a", false, false, 100, 50, 120)
        .expect("malformed CMap must not abort search");
    assert_eq!(search.num_pages, 1);
    assert_eq!(search.truncated, false);
}
