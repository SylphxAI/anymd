use lopdf::{dictionary, Dictionary, Document, Object};
use serde_json::json;

use super::pdf_objects::decoded_string;
use super::{
    extract_page_signals, SignalTextBudget, MAX_ANNOTATIONS_PER_PAGE, MAX_ANNOTATIONS_PER_SOURCE,
    MAX_SIGNAL_TEXT_BYTES, MAX_STRING_BYTES,
};

mod actions;
mod appearance;
mod borders;
mod geometry;

fn fixture_document() -> Document {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-behavior-v1.pdf");
    Document::load(path).expect("load immutable signal fixture")
}

fn document_with_pages(page_dicts: Vec<Dictionary>) -> Document {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let page_ids = page_dicts
        .into_iter()
        .map(|mut page| {
            page.set("Type", "Page");
            page.set("Parent", pages_id);
            document.add_object(page)
        })
        .collect::<Vec<_>>();
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => page_ids.len() as i64,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    document
}

#[test]
fn geometry_and_link_annotation_match_the_frozen_v3014_subset() {
    let document = fixture_document();
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], true, true);
    assert_eq!(signals.warnings, Vec::<String>::new());
    assert_eq!(
        signals.geometry,
        vec![json!({
            "page": 1, "width": 1460.0, "height": 1120.0, "rotation": 90.0,
            "user_unit": 2.0,
            "view_box": { "left": 20.0, "bottom": 30.0, "right": 580.0, "top": 760.0 },
        })]
    );
    assert_eq!(
        signals.annotations,
        vec![json!({
            "page": 1,
            "annotations": [{
                "page": 1, "id": "11R", "subtype": "Link",
                "contents": "  Linked note  ", "url": "https://example.com/a",
                "bounding_box": { "left": 50.0, "bottom": 150.0, "right": 100.0, "top": 200.0 },
            }],
        })]
    );
}

#[test]
fn selected_page_without_annotations_produces_no_placeholder_group() {
    let document = fixture_document();
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[2], false, true);
    assert!(signals.annotations.is_empty());
    assert!(signals.geometry.is_empty());
    assert!(signals.warnings.is_empty());
}

#[test]
fn malformed_geometry_uses_pdfjs_fallback_and_normalization_rules() {
    let document = document_with_pages(vec![
        dictionary! {},
        dictionary! { "MediaBox" => vec![612.into(), 792.into(), 0.into(), 0.into()] },
        dictionary! {
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            "Rotate" => 45,
            "UserUnit" => 0,
        },
        dictionary! {
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            "Rotate" => -90,
        },
        dictionary! { "MediaBox" => vec![0.into(), 0.into(), 0.into(), 0.into()] },
        dictionary! { "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into(), 999.into()] },
        dictionary! {
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 300.into()],
            "CropBox" => vec![10.into(), 10.into(), 10.into(), 20.into()],
        },
    ]);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1, 2, 3, 4, 5, 6, 7], true, false);
    assert_eq!(
        signals.geometry,
        vec![
            json!({"page":1,"width":612.0,"height":792.0,"rotation":0.0,"user_unit":1.0,"view_box":{"left":0.0,"bottom":0.0,"right":612.0,"top":792.0}}),
            json!({"page":2,"width":612.0,"height":792.0,"rotation":0.0,"user_unit":1.0,"view_box":{"left":0.0,"bottom":0.0,"right":612.0,"top":792.0}}),
            json!({"page":3,"width":200.0,"height":300.0,"rotation":0.0,"user_unit":1.0,"view_box":{"left":0.0,"bottom":0.0,"right":200.0,"top":300.0}}),
            json!({"page":4,"width":300.0,"height":200.0,"rotation":270.0,"user_unit":1.0,"view_box":{"left":0.0,"bottom":0.0,"right":200.0,"top":300.0}}),
            json!({"page":5,"width":612.0,"height":792.0,"rotation":0.0,"user_unit":1.0,"view_box":{"left":0.0,"bottom":0.0,"right":612.0,"top":792.0}}),
            json!({"page":6,"width":612.0,"height":792.0,"rotation":0.0,"user_unit":1.0,"view_box":{"left":0.0,"bottom":0.0,"right":612.0,"top":792.0}}),
            json!({"page":7,"width":200.0,"height":300.0,"rotation":0.0,"user_unit":1.0,"view_box":{"left":0.0,"bottom":0.0,"right":200.0,"top":300.0}}),
        ]
    );
}

#[test]
fn malformed_annotations_consume_the_request_wide_work_budget() {
    let page = || {
        dictionary! {
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Annots" => vec![Object::Null; MAX_ANNOTATIONS_PER_PAGE],
        }
    };
    let document = document_with_pages((0..11).map(|_| page()).collect());
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let selected = (1..=11).collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &selected, false, true);
    assert!(signals.annotations.is_empty());
    assert_eq!(signals.warnings, vec![format!(
        "include_annotations: source reached the {MAX_ANNOTATIONS_PER_SOURCE} annotation work limit."
    )]);
}

#[test]
fn oversized_annotation_strings_are_omitted_with_a_warning() {
    let annotation = Object::Dictionary(dictionary! {
        "Subtype" => "Text",
        "Contents" => Object::string_literal("x".repeat(MAX_STRING_BYTES + 1)),
    });
    let document = document_with_pages(vec![dictionary! {
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
        "Annots" => vec![annotation],
    }]);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(signals.annotations[0]["annotations"][0]["subtype"], "Text");
    assert!(signals.annotations[0]["annotations"][0]
        .get("contents")
        .is_none());
    assert_eq!(signals.warnings, vec![format!(
        "include_annotations: annotation strings exceeded the {MAX_STRING_BYTES}-byte field or {MAX_SIGNAL_TEXT_BYTES}-byte source text limit."
    )]);
}

#[test]
fn aggregate_text_exhaustion_stops_later_decode_attempts() {
    let document = Document::with_version("1.7");
    let chunk = Object::string_literal("x".repeat(MAX_STRING_BYTES));
    let sentinel = Object::string_literal("must-not-decode");
    let mut budget = SignalTextBudget::new();
    for _ in 0..(MAX_SIGNAL_TEXT_BYTES / MAX_STRING_BYTES) {
        assert!(decoded_string(&document, &chunk, &mut budget).is_some());
    }
    assert_eq!(budget.remaining, 0);
    assert_eq!(budget.decode_attempts, 32);
    assert!(decoded_string(&document, &sentinel, &mut budget).is_none());
    assert_eq!(budget.decode_attempts, 32);
    assert!(budget.truncated);
}

#[test]
fn popup_inherits_parent_title_and_contents() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-popup-v1.pdf");
    if !path.is_file() {
        return;
    }
    let document = Document::load(&path).expect("load popup fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let anns = signals.annotations[0]["annotations"].as_array().unwrap();
    assert_eq!(anns.len(), 2);
    let popup = anns.iter().find(|a| a["subtype"] == "Popup").unwrap();
    assert_eq!(popup["contents"], "Parent note");
    assert_eq!(popup["title"], "Author");
    assert_eq!(
        popup["bounding_box"],
        json!({"left": 130.0, "bottom": 600.0, "right": 250.0, "top": 670.0})
    );
    let text = anns.iter().find(|a| a["subtype"] == "Text").unwrap();
    assert_eq!(text["contents"], "Parent note");
    assert_eq!(
        text["bounding_box"],
        json!({"left": 100.0, "bottom": 650.0, "right": 122.0, "top": 672.0})
    );
}

#[test]
fn widget_field_t_is_not_projected_as_title() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-widget-field-t-v1.pdf");
    let document = Document::load(path).expect("load widget fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let ann = &signals.annotations[0]["annotations"][0];
    assert_eq!(ann["subtype"], "Widget");
    assert_eq!(ann["contents"], "Widget note");
    assert!(
        ann.get("title").is_none(),
        "Widget /T is fieldName, not title: {ann}"
    );
    assert_eq!(
        ann["bounding_box"],
        serde_json::json!({"left":72.0,"bottom":600.0,"right":120.0,"top":620.0})
    );
}

#[test]
fn group_text_and_popup_inherit_irt_title_and_contents() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-popup-group-irt-v1.pdf");
    if !path.is_file() {
        return;
    }
    let document = Document::load(&path).expect("load group/irt fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let anns = signals.annotations[0]["annotations"].as_array().unwrap();
    assert_eq!(anns.len(), 4);
    let root = anns.iter().find(|a| a["id"] == "5R").unwrap();
    assert_eq!(root["contents"], "Root note");
    assert_eq!(root["title"], "RootAuthor");
    let group = anns.iter().find(|a| a["id"] == "6R").unwrap();
    assert_eq!(group["contents"], "Root note");
    assert_eq!(group["title"], "RootAuthor");
    let group_popup = anns.iter().find(|a| a["id"] == "9R").unwrap();
    assert_eq!(group_popup["contents"], "Root note");
    assert_eq!(group_popup["title"], "RootAuthor");
    assert_eq!(
        group_popup["bounding_box"],
        json!({"left": 230.0, "bottom": 600.0, "right": 350.0, "top": 670.0})
    );
}
