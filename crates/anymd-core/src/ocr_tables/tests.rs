use serde_json::{json, Value};

use crate::ocr_fusion::{OcrPage, OcrWord};

use super::{extract_ocr_tables, merge_ocr_tables, table_index, table_page};

fn word(text: &str, left: f64, bottom: f64, right: f64, top: f64) -> OcrWord {
    OcrWord {
        text: text.into(),
        confidence: Some(0.9),
        reading_order: None,
        region_type: None,
        layout_confidence: None,
        bounding_box: Some(json!({
            "left": left,
            "bottom": bottom,
            "right": right,
            "top": top,
        })),
    }
}

fn page(page: u32, words: Vec<OcrWord>) -> OcrPage {
    OcrPage {
        page,
        text: words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        confidence: Some(0.92),
        words: Some(words),
        language: Some("eng".into()),
        provider: "command".into(),
        source_render_evidence_id: format!("page-{page}-render-scale-2"),
        source_render_scale: Some(2.0),
        source_render_width: Some(1224),
        source_render_height: Some(1584),
        provenance: json!({"engine":"external-command","source":"ocr-provider"}),
        warnings: None,
    }
}

fn ocr_table_page(words_left: f64, page_number: u32) -> OcrPage {
    page(
        page_number,
        vec![
            word("Metric", words_left, 700.0, words_left + 48.0, 710.0),
            word(
                "Value",
                words_left + 120.0,
                700.0,
                words_left + 162.0,
                710.0,
            ),
            word("Revenue", words_left, 680.0, words_left + 60.0, 690.0),
            word("24%", words_left + 120.0, 680.0, words_left + 144.0, 690.0),
        ],
    )
}

#[test]
fn extracts_boxed_two_by_two_with_cells_provenance_and_quality() {
    let result = extract_ocr_tables(&[ocr_table_page(40.0, 1)]);
    let table = &result[0];
    assert_eq!(
        table["rows"],
        json!([["Metric", "Value"], ["Revenue", "24%"]])
    );
    assert_eq!(table["rowCount"], 2);
    assert_eq!(table["colCount"], 2);
    assert_eq!(table["cells"].as_array().map(Vec::len), Some(4));
    assert_eq!(
        table["bounding_box"],
        json!({"left":40.0,"bottom":680.0,"right":202.0,"top":710.0})
    );
    assert_eq!(table["provenance"]["source"], "ocr_text_layer");
    assert_eq!(table["quality"]["cellBoundingBoxCoverage"], 1.0);
    assert_eq!(table["quality"]["signals"], json!(["complete_grid"]));
}

#[test]
fn unboxed_words_and_invalid_boxes_do_not_form_tables() {
    let mut invalid = word("bad", 1.0, 1.0, 2.0, 2.0);
    invalid.bounding_box = Some(json!({"left":1,"bottom":1,"right":1,"top":2}));
    let unboxed = OcrWord {
        text: "plain".into(),
        confidence: None,
        reading_order: None,
        region_type: None,
        layout_confidence: None,
        bounding_box: None,
    };
    assert_eq!(
        extract_ocr_tables(&[page(1, vec![invalid, unboxed])]),
        json!([])
    );
}

#[test]
fn removes_overlap_but_keeps_and_reindexes_distinct_ocr_table() {
    let selectable = json!([
        {
            "page":1,
            "tableIndex":0,
            "rows":[["existing"]],
            "bounding_box":{"left":39,"bottom":679,"right":203,"top":711}
        },
        {
            "page":1,
            "tableIndex":4,
            "rows":[["later"]],
            "bounding_box":{"left":500,"bottom":100,"right":550,"top":120}
        }
    ]);
    let result = merge_ocr_tables(
        &[ocr_table_page(40.0, 1), ocr_table_page(300.0, 1)],
        &selectable,
    );
    let tables = result.as_array().expect("tables");
    assert_eq!(tables.len(), 3);
    assert_eq!(
        tables.iter().map(table_index).collect::<Vec<_>>(),
        vec![0, 4, 5]
    );
    assert_eq!(tables[2]["provenance"]["source"], "ocr_text_layer");
}

#[test]
fn sorts_by_page_then_index_and_rebases_continuation_ids() {
    let selectable = json!([{
        "page":1,
        "tableIndex":0,
        "rows":[["selectable"]],
        "bounding_box":{"left":500,"bottom":100,"right":550,"top":120}
    }]);
    let result = merge_ocr_tables(
        &[ocr_table_page(300.0, 2), ocr_table_page(300.0, 1)],
        &selectable,
    );
    let tables = result.as_array().expect("tables");
    assert_eq!(
        tables
            .iter()
            .map(|table| (table_page(table), table_index(table)))
            .collect::<Vec<_>>(),
        vec![(1, 0), (1, 1), (2, 0)]
    );
    assert_eq!(tables[1]["continuation"]["nextTableId"], "p2-table-1");
    assert_eq!(tables[2]["continuation"]["previousTableId"], "p1-table-2");
    assert_eq!(
        tables[1]["continuation"]["groupId"],
        "table-continuation-p1-table-2-p2-table-1"
    );
}

#[test]
fn non_array_selectable_input_is_an_empty_collection() {
    let result = merge_ocr_tables(&[ocr_table_page(40.0, 2)], &Value::Null);
    assert_eq!(result[0]["page"], 2);
    assert_eq!(result[0]["tableIndex"], 0);
}
