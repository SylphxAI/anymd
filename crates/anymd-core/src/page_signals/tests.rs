use super::*;
use lopdf::{dictionary, Dictionary};

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
fn dest_only_link_keeps_pdfjs_page_ref_and_name_shape() {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let page_two_id = document.new_object_id();
    let link_id = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![72.into(), 500.into(), 140.into(), 530.into()],
        "Dest" => vec![Object::Reference(page_two_id), Object::Name("Fit".into())],
    });
    let page_one_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Annots" => vec![Object::Reference(link_id)],
    });
    document.objects.insert(
        page_two_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        }),
    );
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_one_id), Object::Reference(page_two_id)],
            "Count" => 2,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(signals.warnings, Vec::<String>::new());
    let annotations = &signals.annotations[0]["annotations"].as_array().unwrap()[0];
    assert_eq!(annotations["subtype"], json!("Link"));
    assert_eq!(
        annotations["dest"],
        json!([{ "num": page_two_id.0, "gen": page_two_id.1 }, { "name": "Fit" }])
    );
}

#[test]
fn goto_action_dest_is_exposed_like_pdfjs() {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let page_two_id = document.new_object_id();
    let action = dictionary! {
        "S" => "GoTo",
        "D" => vec![
            Object::Reference(page_two_id),
            Object::Name("FitH".into()),
            700.into(),
        ],
    };
    let link_id = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![72.into(), 500.into(), 140.into(), 530.into()],
        "A" => action,
    });
    let page_one_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Annots" => vec![Object::Reference(link_id)],
    });
    document.objects.insert(
        page_two_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        }),
    );
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_one_id), Object::Reference(page_two_id)],
            "Count" => 2,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(signals.warnings, Vec::<String>::new());
    let annotations = &signals.annotations[0]["annotations"].as_array().unwrap()[0];
    assert_eq!(annotations["subtype"], json!("Link"));
    assert_eq!(
        annotations["dest"],
        json!([
            { "num": page_two_id.0, "gen": page_two_id.1 },
            { "name": "FitH" },
            700
        ])
    );
}

#[test]
fn goto_action_wins_over_explicit_dest_like_pdfjs() {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let page_two_id = document.new_object_id();
    let action = dictionary! {
        "S" => "GoTo",
        "D" => vec![
            Object::Reference(page_two_id),
            Object::Name("FitH".into()),
            10.into(),
        ],
    };
    let link_id = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![72.into(), 500.into(), 140.into(), 530.into()],
        "Dest" => vec![Object::Reference(page_two_id), Object::Name("Fit".into())],
        "A" => action,
    });
    let page_one_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Annots" => vec![Object::Reference(link_id)],
    });
    document.objects.insert(
        page_two_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        }),
    );
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_one_id), Object::Reference(page_two_id)],
            "Count" => 2,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let annotations = &signals.annotations[0]["annotations"].as_array().unwrap()[0];
    assert_eq!(
        annotations["dest"],
        json!([
            { "num": page_two_id.0, "gen": page_two_id.1 },
            { "name": "FitH" },
            10
        ])
    );
    assert!(annotations.get("url").is_none());
}

#[test]
fn uri_action_suppresses_dest_and_launch_maps_file_to_url() {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let page_two_id = document.new_object_id();
    let uri_link = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![72.into(), 500.into(), 140.into(), 530.into()],
        "Dest" => vec![Object::Reference(page_two_id), Object::Name("Fit".into())],
        "A" => dictionary! {
            "S" => "URI",
            "URI" => Object::string_literal("https://example.com/x"),
        },
    });
    let launch_link = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => vec![150.into(), 500.into(), 220.into(), 530.into()],
        "A" => dictionary! {
            "S" => "Launch",
            "F" => Object::string_literal("evil.exe"),
        },
    });
    let page_one_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![Object::Reference(uri_link), Object::Reference(launch_link)],
    });
    document.objects.insert(
        page_two_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_one_id), Object::Reference(page_two_id)],
            "Count" => 2,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let annotations = signals.annotations[0]["annotations"].as_array().unwrap();
    assert_eq!(annotations[0]["url"], json!("https://example.com/x"));
    assert!(annotations[0].get("dest").is_none());
    assert_eq!(annotations[1]["url"], json!("evil.exe"));
    assert!(annotations[1].get("dest").is_none());
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
fn text_annotation_without_appearance_uses_pdfjs_icon_box() {
    let annotation = Object::Dictionary(dictionary! {
        "Subtype" => "Text",
        "Contents" => "Sticky note",
        "T" => "Author",
        "Rect" => vec![120.into(), 680.into(), 140.into(), 700.into()],
    });
    let document = document_with_pages(vec![dictionary! {
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![annotation],
    }]);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["bounding_box"],
        json!({"left": 120.0, "bottom": 678.0, "right": 142.0, "top": 700.0})
    );
    assert_eq!(signals.annotations[0]["annotations"][0]["subtype"], "Text");
}

#[test]
fn freetext_annotation_keeps_raw_rect_box() {
    let annotation = Object::Dictionary(dictionary! {
        "Subtype" => "FreeText",
        "Contents" => "Hello FreeText",
        "Rect" => vec![100.into(), 600.into(), 250.into(), 650.into()],
    });
    let document = document_with_pages(vec![dictionary! {
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![annotation],
    }]);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["bounding_box"],
        json!({"left": 100.0, "bottom": 600.0, "right": 250.0, "top": 650.0})
    );
}

#[test]
fn launch_file_dict_prefers_uf_like_pdfjs() {
    let annotation = Object::Dictionary(dictionary! {
        "Subtype" => "Link",
        "Rect" => vec![72.into(), 500.into(), 140.into(), 530.into()],
        "A" => dictionary! {
            "S" => "Launch",
            "F" => dictionary! {
                "F" => Object::string_literal("report.pdf"),
                "UF" => Object::string_literal("report-u.pdf"),
            },
        },
    });
    let document = document_with_pages(vec![dictionary! {
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![annotation],
    }]);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["url"],
        "report-u.pdf"
    );
    assert!(signals.annotations[0]["annotations"][0]
        .get("dest")
        .is_none());
}

#[test]
fn gotor_appends_remote_dest_json_like_pdfjs() {
    let annotation = Object::Dictionary(dictionary! {
        "Subtype" => "Link",
        "Rect" => vec![72.into(), 500.into(), 140.into(), 530.into()],
        "A" => dictionary! {
            "S" => "GoToR",
            "F" => Object::string_literal("other.pdf"),
            "D" => vec![Object::Integer(0), Object::Name("Fit".into())],
            "NewWindow" => true,
        },
    });
    let document = document_with_pages(vec![dictionary! {
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![annotation],
    }]);
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["url"],
        r#"other.pdf#[0,{"name":"Fit"}]"#
    );
    assert!(signals.annotations[0]["annotations"][0]
        .get("dest")
        .is_none());
}

#[test]
fn gotor_named_dest_string_and_name_token_append_like_pdfjs() {
    for (fixture, _) in [
        (
            "../../test/fixtures/differential/v3014-annotation-gotor-named-string-v1.pdf",
            "string",
        ),
        (
            "../../test/fixtures/differential/v3014-annotation-gotor-named-name-v1.pdf",
            "name",
        ),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture);
        if !path.is_file() {
            continue;
        }
        let document = Document::load(&path).expect("load gotor named fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        assert_eq!(
            signals.annotations[0]["annotations"][0]["url"],
            "other.pdf#Chapter1"
        );
        assert!(signals.annotations[0]["annotations"][0]
            .get("dest")
            .is_none());
    }
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
fn popup_zero_size_rect_omits_bounding_box() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-popup-zerosize-v1.pdf");
    if !path.is_file() {
        return;
    }
    let document = Document::load(&path).expect("load zero-size popup fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let anns = signals.annotations[0]["annotations"].as_array().unwrap();
    let popup = anns.iter().find(|a| a["subtype"] == "Popup").unwrap();
    assert!(
        popup.get("bounding_box").is_none(),
        "zero-size popup must omit bounding_box"
    );
    assert_eq!(popup["contents"], "Parent note");
    assert_eq!(popup["title"], "Author");
    let text = anns.iter().find(|a| a["subtype"] == "Text").unwrap();
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
fn link_uri_domain_gets_trailing_slash_like_pdfjs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-link-uri-domain-v1.pdf");
    let document = Document::load(path).expect("load link domain fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["url"],
        json!("https://example.com/")
    );
}

#[test]
fn link_uri_name_becomes_slash_name_like_pdfjs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-link-uri-name-v1.pdf");
    let document = Document::load(path).expect("load uri name fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["url"],
        json!("/Example")
    );
}

#[test]
fn link_aa_u_uri_projects_url_like_pdfjs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-link-aa-u-uri-v1.pdf");
    let document = Document::load(path).expect("load aa/u fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    assert_eq!(
        signals.annotations[0]["annotations"][0]["url"],
        json!("https://example.com/aa")
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

#[test]
fn text_with_appearance_keeps_raw_rect_even_if_stream_empty() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-ap-v1.pdf");
    if path.is_file() {
        let document = Document::load(&path).expect("load text ap fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], "Text");
        assert_eq!(
            ann["bounding_box"],
            json!({"left": 100.0, "bottom": 600.0, "right": 200.0, "top": 700.0})
        );
    }
    let empty = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-emptyap-v1.pdf");
    if empty.is_file() {
        let document = Document::load(&empty).expect("load empty ap fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], "Text");
        assert_eq!(
            ann["bounding_box"],
            json!({"left": 50.0, "bottom": 600.0, "right": 150.0, "top": 700.0})
        );
    }
}

#[test]
fn text_named_appearance_requires_as_for_raw_rect() {
    let with_as = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-namedap-v1.pdf");
    if with_as.is_file() {
        let document = Document::load(&with_as).expect("load named ap fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(
            ann["bounding_box"],
            json!({"left": 80.0, "bottom": 600.0, "right": 180.0, "top": 700.0})
        );
    }
    let no_as = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-namedap-noas-v1.pdf");
    if no_as.is_file() {
        let document = Document::load(&no_as).expect("load named ap no-as fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        // pdf.js icon box: bottom = top-22, right = left+22
        assert_eq!(
            ann["bounding_box"],
            json!({"left": 90.0, "bottom": 688.0, "right": 112.0, "top": 710.0})
        );
    }
}

#[test]
fn text_no_appearance_normalizes_inverted_rect_before_icon_box() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-annotation-text-inverted-v1.pdf");
    if !path.is_file() {
        return;
    }
    let document = Document::load(&path).expect("load inverted text fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let signals = extract_page_signals(&document, &pages, &[1], false, true);
    let ann = &signals.annotations[0]["annotations"][0];
    assert_eq!(ann["subtype"], "Text");
    assert_eq!(ann["contents"], "Inverted");
    assert_eq!(
        ann["bounding_box"],
        json!({"left": 100.0, "bottom": 678.0, "right": 122.0, "top": 700.0})
    );
}

#[test]
fn polyline_polygon_nonintersecting_rect_uses_vertices_bbox() {
    let cases = [
        (
            "v3014-annotation-polyline-l-bbox-v1.pdf",
            "PolyLine",
            json!({"left": 98.0, "bottom": 98.0, "right": 202.0, "top": 202.0}),
        ),
        (
            "v3014-annotation-polygon-l-bbox-v1.pdf",
            "Polygon",
            json!({"left": 48.0, "bottom": 48.0, "right": 122.0, "top": 122.0}),
        ),
        (
            "v3014-annotation-polyline-border2-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load polyline/polygon fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype);
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ink_nonintersecting_rect_uses_inklist_bbox() {
    let cases = [
        (
            "v3014-annotation-ink-l-bbox-v1.pdf",
            json!({"left": 98.0, "bottom": 98.0, "right": 182.0, "top": 202.0}),
        ),
        (
            "v3014-annotation-ink-multistroke-v1.pdf",
            json!({"left": 8.0, "bottom": 48.0, "right": 92.0, "top": 122.0}),
        ),
        (
            "v3014-annotation-ink-border2-v1.pdf",
            json!({"left": 26.0, "bottom": 26.0, "right": 104.0, "top": 94.0}),
        ),
    ];
    for (fixture, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load ink fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], "Ink");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn tiny_rect_border_width_clamp_matches_pdfjs() {
    let cases = [
        (
            "v3014-annotation-polyline-clamp-w2-v1.pdf",
            "PolyLine",
            json!({"left": 8.0, "bottom": 8.0, "right": 102.0, "top": 82.0}),
        ),
        (
            "v3014-annotation-line-clamp-w2-v1.pdf",
            "Line",
            json!({"left": 7.0, "bottom": 7.0, "right": 103.0, "top": 83.0}),
        ),
        (
            "v3014-annotation-ink-clamp-w2-v1.pdf",
            "Ink",
            json!({"left": 28.0, "bottom": 28.0, "right": 102.0, "top": 92.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load clamp fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_array_width_drives_line_polyline_ink_boxes() {
    let cases = [
        (
            "v3014-annotation-polyline-border-array-w2-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
        (
            "v3014-annotation-line-border-array-w2-v1.pdf",
            "Line",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-ink-border-array-w3-v1.pdf",
            "Ink",
            json!({"left": 24.0, "bottom": 24.0, "right": 106.0, "top": 96.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load border array fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_bs_preference_over_border_array_width() {
    // Border [0 0 9] would expand much more; BS/W must win.
    let cases = [
        (
            "v3014-annotation-polyline-border-bs-pref-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
        (
            "v3014-annotation-line-border-bs-pref-v1.pdf",
            "Line",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-ink-border-bs-pref-v1.pdf",
            "Ink",
            json!({"left": 24.0, "bottom": 24.0, "right": 106.0, "top": 96.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load border BS preference fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_bs_nondict_does_not_fall_through_to_border_array() {
    // BS null with Border [0 0 9] must keep default width 1, not Border[2]=9.
    let cases = [
        (
            "v3014-annotation-polyline-border-bs-null-v1.pdf",
            "PolyLine",
            json!({"left": 8.0, "bottom": 8.0, "right": 102.0, "top": 82.0}),
        ),
        (
            "v3014-annotation-line-border-bs-null-v1.pdf",
            "Line",
            json!({"left": 7.0, "bottom": 7.0, "right": 103.0, "top": 83.0}),
        ),
        (
            "v3014-annotation-ink-border-bs-null-v1.pdf",
            "Ink",
            json!({"left": 28.0, "bottom": 28.0, "right": 102.0, "top": 92.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load border BS nondict fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_array_short_keeps_default_width() {
    // Border length < 3 (no BS) must keep default drawing width 1.
    let cases = [
        (
            "v3014-annotation-polyline-border-short-v1.pdf",
            "PolyLine",
            json!({"left": 8.0, "bottom": 8.0, "right": 102.0, "top": 82.0}),
        ),
        (
            "v3014-annotation-line-border-empty-v1.pdf",
            "Line",
            json!({"left": 7.0, "bottom": 7.0, "right": 103.0, "top": 83.0}),
        ),
        (
            "v3014-annotation-ink-border-short-v1.pdf",
            "Ink",
            json!({"left": 28.0, "bottom": 28.0, "right": 102.0, "top": 92.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load short Border fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_bs_wrong_type_ignores_w_and_border() {
    // BS Type not Border with W=9 and Border[2]=5 must keep default width 1.
    let cases = [
        (
            "v3014-annotation-polyline-border-bs-wrong-type-v1.pdf",
            "PolyLine",
            json!({"left": 8.0, "bottom": 8.0, "right": 102.0, "top": 82.0}),
        ),
        (
            "v3014-annotation-line-border-bs-wrong-type-v1.pdf",
            "Line",
            json!({"left": 7.0, "bottom": 7.0, "right": 103.0, "top": 83.0}),
        ),
        (
            "v3014-annotation-ink-border-bs-wrong-type-v1.pdf",
            "Ink",
            json!({"left": 28.0, "bottom": 28.0, "right": 102.0, "top": 92.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load BS wrong-type fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn border_zero_size_rect_bypasses_width_clamp() {
    // Zero-dimension Rect + BS/W=2 must keep width 2 (no clamp).
    let cases = [
        (
            "v3014-annotation-polyline-zero-h-w2-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
        (
            "v3014-annotation-line-zero-h-w2-v1.pdf",
            "Line",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-ink-zero-w-w2-v1.pdf",
            "Ink",
            json!({"left": 26.0, "bottom": 26.0, "right": 104.0, "top": 94.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load zero-size clamp-bypass fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn appearance_present_skips_line_polyline_ink_geometry_expansion() {
    // AP/N present => keep raw Rect even when L/vertices would expand farther.
    let cases = [
        (
            "v3014-annotation-line-ap-bbox-v1.pdf",
            "Line",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-polyline-ap-bbox-v1.pdf",
            "PolyLine",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-ink-ap-bbox-v1.pdf",
            "Ink",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load appearance bbox fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ap_n_nonstream_still_expands_line_polyline_ink_geometry() {
    // AP/N null or name is not appearance => expand geometry with BS/W=2.
    let cases = [
        (
            "v3014-annotation-line-ap-n-null-v1.pdf",
            "Line",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-polyline-ap-n-name-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
        (
            "v3014-annotation-ink-ap-n-null-v1.pdf",
            "Ink",
            json!({"left": 26.0, "bottom": 26.0, "right": 104.0, "top": 94.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load AP non-stream fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ap_named_state_as_selection_for_line_geometry() {
    let cases = [
        (
            "v3014-annotation-line-ap-as-on-v1.pdf",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-line-ap-as-missing-v1.pdf",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
        (
            "v3014-annotation-line-ap-as-invalid-v1.pdf",
            json!({"left": 4.0, "bottom": 4.0, "right": 106.0, "top": 86.0}),
        ),
    ];
    for (fixture, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load AP named-state fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], "Line", "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ap_named_state_polyline_ink_breadth() {
    let cases = [
        (
            "v3014-annotation-polyline-ap-as-on-v1.pdf",
            "PolyLine",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-ink-ap-as-missing-v1.pdf",
            "Ink",
            json!({"left": 26.0, "bottom": 26.0, "right": 104.0, "top": 94.0}),
        ),
        (
            "v3014-annotation-polyline-ap-as-invalid-v1.pdf",
            "PolyLine",
            json!({"left": 6.0, "bottom": 6.0, "right": 104.0, "top": 84.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load named-state polyline/ink fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn ap_named_state_square_circle_breadth() {
    let cases = [
        (
            "v3014-annotation-square-ap-as-on-v1.pdf",
            "Square",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-circle-ap-as-missing-v1.pdf",
            "Circle",
            json!({"left": 50.0, "bottom": 60.0, "right": 150.0, "top": 160.0}),
        ),
        (
            "v3014-annotation-square-ap-as-invalid-v1.pdf",
            "Square",
            json!({"left": 10.0, "bottom": 20.0, "right": 110.0, "top": 120.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load named-state square/circle fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn highlight_quadpoints_bbox() {
    let cases = [
        (
            "v3014-annotation-highlight-quad-noap-v1.pdf",
            "Highlight",
            json!({"left": 10.0, "bottom": 10.0, "right": 100.0, "top": 80.0}),
        ),
        (
            "v3014-annotation-highlight-ap-noext-v1.pdf",
            "Highlight",
            json!({"left": 10.0, "bottom": 10.0, "right": 100.0, "top": 80.0}),
        ),
        (
            "v3014-annotation-highlight-ap-ext-v1.pdf",
            "Highlight",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load highlight fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn text_markup_quadpoints_breadth() {
    let cases = [
        (
            "v3014-annotation-underline-quad-noap-v1.pdf",
            "Underline",
            json!({"left": 20.0, "bottom": 20.0, "right": 120.0, "top": 90.0}),
        ),
        (
            "v3014-annotation-squiggly-quad-noap-v1.pdf",
            "Squiggly",
            json!({"left": 30.0, "bottom": 6.666666666666668, "right": 130.0, "top": 53.33333333333333}),
        ),
        (
            "v3014-annotation-strikeout-quad-noap-v1.pdf",
            "StrikeOut",
            json!({"left": 40.0, "bottom": 40.0, "right": 140.0, "top": 110.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load text-markup fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}

#[test]
fn text_markup_with_appearance_keeps_rect() {
    let cases = [
        (
            "v3014-annotation-underline-ap-keeps-rect-v1.pdf",
            "Underline",
            json!({"left": 200.0, "bottom": 200.0, "right": 300.0, "top": 300.0}),
        ),
        (
            "v3014-annotation-squiggly-ap-keeps-rect-v1.pdf",
            "Squiggly",
            json!({"left": 150.0, "bottom": 160.0, "right": 250.0, "top": 260.0}),
        ),
        (
            "v3014-annotation-strikeout-ap-keeps-rect-v1.pdf",
            "StrikeOut",
            json!({"left": 100.0, "bottom": 110.0, "right": 210.0, "top": 220.0}),
        ),
    ];
    for (fixture, subtype, expected) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        assert!(path.is_file(), "missing fixture {fixture}");
        let document = Document::load(&path).expect("load text-markup AP fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let signals = extract_page_signals(&document, &pages, &[1], false, true);
        let ann = &signals.annotations[0]["annotations"][0];
        assert_eq!(ann["subtype"], subtype, "fixture {fixture}");
        assert_eq!(ann["bounding_box"], expected, "fixture {fixture}");
    }
}
