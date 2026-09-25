use crate::blocks::*;
use crate::extract::*;
use crate::margins::*;
use crate::reading::*;
use crate::rows::*;

fn glyphs(line: &str, x: f64, base: f64, size: f64, advance: f64, word_gap: f64) -> Vec<Glyph> {
    let mut out = Vec::new();
    let mut cursor = x;
    for ch in line.chars() {
        if ch == ' ' {
            cursor += word_gap;
            continue;
        }
        out.push(Glyph {
            x0: cursor,
            x1: cursor + advance,
            base,
            size,
            text: ch.to_string(),
            space: false,
        });
        cursor += advance;
    }
    out
}

#[test]
fn infers_word_spaces_from_gaps_without_space_glyphs() {
    let row = glyphs("The dominant sequence", 72.0, 700.0, 10.0, 5.0, 3.3);
    let segments = segments_of_row(row);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text, "The dominant sequence");
}

#[test]
fn tight_kerning_does_not_split_words() {
    let row = glyphs("Transformer", 72.0, 700.0, 10.0, 5.0, 0.0)
        .into_iter()
        .enumerate()
        .map(|(i, mut g)| {
            let shift = if i % 2 == 0 { -0.4 } else { 0.6 };
            g.x0 += shift;
            g.x1 += shift;
            g
        })
        .collect();
    assert_eq!(segments_of_row(row)[0].text, "Transformer");
}

#[test]
fn cjk_glyphs_do_not_get_spaces() {
    let row = glyphs("注意力机制", 72.0, 700.0, 10.0, 10.0, 0.0)
        .into_iter()
        .enumerate()
        .map(|(i, mut g)| {
            g.x0 += i as f64 * 0.8;
            g.x1 += i as f64 * 0.8;
            g
        })
        .collect();
    assert_eq!(segments_of_row(row)[0].text, "注意力机制");
}

#[test]
fn large_gaps_split_segments() {
    let mut row = glyphs("Model", 72.0, 700.0, 10.0, 5.0, 3.0);
    row.extend(glyphs("BLEU", 200.0, 700.0, 10.0, 5.0, 3.0));
    let segments = segments_of_row(row);
    assert_eq!(
        segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        ["Model", "BLEU"]
    );
}

#[test]
fn superscripts_stay_on_their_row() {
    let mut glyph_list = glyphs("word", 72.0, 700.0, 10.0, 5.0, 3.0);
    glyph_list.extend(glyphs("2", 92.5, 703.5, 7.0, 3.5, 0.0));
    let rows = rows_of(glyph_list);
    assert_eq!(rows.len(), 1);
}

fn mono_glyphs(line: &str, base: f64) -> Vec<Glyph> {
    line.chars()
        .enumerate()
        .filter(|(_, ch)| *ch != ' ')
        .map(|(i, ch)| Glyph {
            x0: 72.0 + i as f64 * 6.0,
            x1: 78.0 + i as f64 * 6.0,
            base,
            size: 10.0,
            text: ch.to_string(),
            space: false,
        })
        .collect()
}

#[test]
fn monospace_lines_keep_their_breaks() {
    let lines = [
        "Wireless Noise-Cancelling",
        "Headphones - Premium Black",
        "AUDIO-5521 1 @ $349.99",
        "Member Discount $-50.00",
    ];
    let mut segments = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut row = mono_glyphs(line, 700.0 - i as f64 * 12.0);
        // Word gaps are one monospace cell.
        for glyph in &mut row {
            glyph.text = glyph.text.clone();
        }
        segments.extend(segments_of_row(row));
    }
    assert!(segments.iter().any(|s| s.mono == Some(true)));
    let mut blocks = Vec::new();
    region_blocks(segments, 10.0, &mut PageTables::none(), &mut blocks);
    match &blocks[..] {
        [Block::Paragraph { text, .. }] => assert_eq!(text, &lines.join("\n")),
        other => panic!("expected one line-preserving block, got {other:?}"),
    }
}

#[test]
fn dehyphenates_line_breaks() {
    let mut text = String::from("trans-");
    join_line(&mut text, "duction models");
    assert_eq!(text, "transduction models");
    let mut keep = String::from("state-of-the-");
    join_line(&mut keep, "Art");
    assert_eq!(keep, "state-of-the- Art");
    let mut compound = String::from("a left-to-");
    join_line(&mut compound, "right model");
    assert_eq!(compound, "a left-to-right model");
}

#[test]
fn detects_numbered_headings() {
    assert_eq!(numbered_heading_level("3.2 Attention"), Some(3));
    assert_eq!(numbered_heading_level("1 Introduction"), Some(2));
    assert_eq!(
        numbered_heading_level("3.2.1 Scaled Dot-Product Attention"),
        Some(4)
    );
    assert_eq!(
        numbered_heading_level("2 GPUs were used for training."),
        None
    );
    assert_eq!(numbered_heading_level("100 Epochs"), None);
}

#[test]
fn recognizes_page_numbers() {
    assert!(is_page_number("12"));
    assert!(is_page_number("Page 3 of 10"));
    assert!(is_page_number("- 4 -"));
    assert!(is_page_number("xii"));
    assert!(!is_page_number("Results"));
}

/// Words spread evenly over the segment's extent.
fn words_of(text: &str, x0: f64, x1: f64) -> Vec<Word> {
    let total = text.chars().count().max(1) as f64;
    let per = (x1 - x0) / total;
    let mut out = Vec::new();
    let mut offset = 0usize;
    for word in text.split(' ') {
        let len = word.chars().count();
        if len > 0 {
            out.push(Word {
                x0: x0 + offset as f64 * per,
                x1: x0 + (offset + len) as f64 * per,
                text: word.into(),
            });
        }
        offset += len + 1;
    }
    out
}

fn segment(text: &str, x0: f64, x1: f64, base: f64) -> Segment {
    Segment {
        x0,
        x1,
        base,
        top: base + 8.0,
        bottom: base - 2.0,
        size: 10.0,
        text: text.into(),
        mono: None,
        words: words_of(text, x0, x1),
        table: None,
    }
}

#[test]
fn two_columns_read_left_then_right() {
    let long = "a line of running text that fills the column width";
    let mut segments = vec![segment("Title Of The Paper", 150.0, 450.0, 760.0)];
    for i in 0..10 {
        let base = 700.0 - i as f64 * 12.0;
        segments.push(segment(&format!("L{i} {long}"), 72.0, 290.0, base));
        segments.push(segment(&format!("R{i} {long}"), 320.0, 540.0, base));
    }
    let regions = reading_regions(segments, 10.0, 0);
    let order: Vec<String> = regions
        .iter()
        .flat_map(|region| group_rows(region.clone()))
        .map(|row| row_text(&row).chars().take(3).collect())
        .collect();
    assert_eq!(order[0], "Tit");
    assert_eq!(order[1], "L0 ");
    assert_eq!(order[10], "L9 ");
    assert_eq!(order[11], "R0 ");
}

#[test]
fn aligned_cells_become_a_table() {
    let rows = vec![
        vec![
            segment("Model", 72.0, 110.0, 700.0),
            segment("BLEU", 200.0, 230.0, 700.0),
            segment("Cost", 300.0, 330.0, 700.0),
        ],
        vec![
            segment("ByteNet", 72.0, 120.0, 688.0),
            segment("23.75", 200.0, 228.0, 688.0),
            segment("1.0", 300.0, 318.0, 688.0),
        ],
        vec![
            segment("GNMT", 72.0, 105.0, 676.0),
            segment("24.6", 202.0, 226.0, 676.0),
            segment("2.3", 300.0, 318.0, 676.0),
        ],
    ];
    let mut blocks = Vec::new();
    region_blocks(
        rows.into_iter().flatten().collect(),
        10.0,
        &mut PageTables::none(),
        &mut blocks,
    );
    match &blocks[..] {
        [Block::Table(table)] => {
            assert_eq!(table[0], ["Model", "BLEU", "Cost"]);
            assert_eq!(table[2], ["GNMT", "24.6", "2.3"]);
        }
        other => panic!("expected one table, got {other:?}"),
    }
}
