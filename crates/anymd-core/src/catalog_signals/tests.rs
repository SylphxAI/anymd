use super::*;
use lopdf::dictionary;

fn document(catalog_extra: Dictionary) -> Document {
    let mut document = Document::with_version("1.7");
    let pages =
        document.add_object(dictionary! {"Type"=>"Pages","Kids"=>Vec::<Object>::new(),"Count"=>0});
    let mut catalog = dictionary! {"Type"=>"Catalog","Pages"=>pages};
    catalog.extend(&catalog_extra);
    let root = document.add_object(catalog);
    document.trailer.set("Root", root);
    document
}

#[test]
fn expands_number_tree_labels_and_mark_info() {
    let doc = document(dictionary! {
        "PageLabels"=>dictionary!{"Nums"=>vec![0.into(),Object::Dictionary(dictionary!{"S"=>"r"}),2.into(),Object::Dictionary(dictionary!{"P"=>Object::string_literal("A-"),"S"=>"D","St"=>3})]},
        "MarkInfo"=>dictionary!{"Marked"=>true,"Suspects"=>false},
    });
    let out = extract_catalog_signals(
        &doc,
        None,
        4,
        CatalogSignalRequest {
            page_labels: true,
            permissions: true,
            outline: false,
        },
    );
    assert_eq!(
        out.page_labels,
        Some(vec!["i".into(), "ii".into(), "A-3".into(), "A-4".into()])
    );
    assert_eq!(
        serde_json::to_value(out.mark_info).unwrap(),
        serde_json::json!({"Marked":true,"UserProperties":false,"Suspects":false})
    );
}

#[test]
fn preserves_nested_outline_styles_and_safe_actions() {
    let mut doc = document(dictionary! {});
    let child=doc.add_object(dictionary!{"Title"=>Object::string_literal("Child"),"A"=>dictionary!{"S"=>"URI","URI"=>Object::string_literal("https://example.com")}});
    let first=doc.add_object(dictionary!{"Title"=>Object::string_literal("Root"),"F"=>3,"C"=>vec![1.into(),0.into(),0.into()],"First"=>child});
    let outlines = doc.add_object(dictionary! {"First"=>first});
    doc.catalog_mut().unwrap().set("Outlines", outlines);
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["title"], "Root");
    assert_eq!(value[0]["bold"], true);
    assert_eq!(value[0]["items"][0]["url"], "https://example.com/");
}

#[test]
fn permission_bits_match_pdfjs_p_flag_enumeration() {
    let facts = EncryptionFacts {
        permissions: Some(4 | 8 | 256),
        filter_name: Some("Standard".to_string()),
    };
    assert_eq!(
        extract_permissions(Some(facts)),
        Some(vec!["print".into(), "modify".into(), "fill_forms".into()])
    );
}

#[test]
fn mark_info_defaults_missing_and_non_boolean_keys_to_false() {
    let doc = document(dictionary! {
        "MarkInfo"=>dictionary!{"Marked"=>true,"Suspects"=>1},
    });
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: true,
            outline: false,
        },
    );
    assert_eq!(
        serde_json::to_value(out.mark_info).unwrap(),
        serde_json::json!({"Marked":true,"UserProperties":false,"Suspects":false})
    );
}

#[test]
fn outline_without_action_retains_pdfjs_null_destination() {
    let mut doc = document(dictionary! {});
    let first = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("Plain"),
    });
    let outlines = doc.add_object(dictionary! {"First"=>first});
    doc.catalog_mut().unwrap().set("Outlines", outlines);
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["dest"], serde_json::Value::Null);
    assert_eq!(value[0]["bold"], false);
    assert_eq!(value[0]["color"], serde_json::json!([0, 0, 0]));
}

#[test]
fn outline_gotor_absolute_url_appends_remote_dest_like_pdfjs() {
    let mut doc = Document::with_version("1.7");
    let pages = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => Vec::<Object>::new(),
        "Count" => 0
    });
    let action = doc.add_object(dictionary! {
        "S" => "GoToR",
        "F" => Object::string_literal("https://example.com/docs/other.pdf"),
        "D" => Object::string_literal("Chapter1"),
    });
    let item = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Remote"),
        "A" => action,
    });
    let outlines = doc.add_object(dictionary! {
        "Type" => "Outlines",
        "First" => item,
        "Last" => item,
        "Count" => 1
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages,
        "Outlines" => outlines,
    });
    doc.trailer.set("Root", catalog);
    let out = extract_catalog_signals(
        &doc,
        None,
        1,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["title"], "Remote");
    assert_eq!(
        value[0]["url"],
        "https://example.com/docs/other.pdf#Chapter1"
    );
    assert_eq!(value[0]["dest"], serde_json::Value::Null);
}

#[test]
fn outline_gotor_relative_file_is_dropped_like_pdfjs() {
    let mut doc = Document::with_version("1.7");
    let pages = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => Vec::<Object>::new(),
        "Count" => 0
    });
    let action = doc.add_object(dictionary! {
        "S" => "GoToR",
        "F" => Object::string_literal("other.pdf"),
        "D" => Object::string_literal("Chapter1"),
    });
    let item = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Remote"),
        "A" => action,
    });
    let outlines = doc.add_object(dictionary! {
        "Type" => "Outlines",
        "First" => item,
        "Last" => item,
        "Count" => 1
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages,
        "Outlines" => outlines,
    });
    doc.trailer.set("Root", catalog);
    let out = extract_catalog_signals(
        &doc,
        None,
        1,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["title"], "Remote");
    assert!(value[0].get("url").is_none());
    assert_eq!(value[0]["dest"], serde_json::Value::Null);
}

#[test]
fn outline_launch_absolute_url_matches_gotor_like_pdfjs() {
    let mut doc = Document::with_version("1.7");
    let pages = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => Vec::<Object>::new(),
        "Count" => 0
    });
    let action = doc.add_object(dictionary! {
        "S" => "Launch",
        "F" => Object::string_literal("https://example.com/docs/other.pdf"),
        "D" => Object::string_literal("Chapter1"),
    });
    let item = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Launch Remote"),
        "A" => action,
    });
    let outlines = doc.add_object(dictionary! {
        "Type" => "Outlines",
        "First" => item,
        "Last" => item,
        "Count" => 1
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages,
        "Outlines" => outlines,
    });
    doc.trailer.set("Root", catalog);
    let out = extract_catalog_signals(
        &doc,
        None,
        1,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["title"], "Launch Remote");
    assert_eq!(
        value[0]["url"],
        "https://example.com/docs/other.pdf#Chapter1"
    );
    assert_eq!(value[0]["dest"], serde_json::Value::Null);
}

#[test]
fn outline_launch_filespec_prefers_uf_like_pdfjs() {
    let mut doc = Document::with_version("1.7");
    let pages = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => Vec::<Object>::new(),
        "Count" => 0
    });
    let filespec = dictionary! {
        "UF" => Object::string_literal("https://example.com/docs/uf.pdf"),
        "F" => Object::string_literal("https://example.com/docs/f.pdf"),
    };
    let action = doc.add_object(dictionary! {
        "S" => "Launch",
        "F" => filespec,
        "D" => Object::string_literal("Intro"),
    });
    let item = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Launch Filespec"),
        "Dest" => Object::string_literal("ShouldBeSuppressed"),
        "A" => action,
    });
    let outlines = doc.add_object(dictionary! {
        "Type" => "Outlines",
        "First" => item,
        "Last" => item,
        "Count" => 1
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages,
        "Outlines" => outlines,
    });
    doc.trailer.set("Root", catalog);
    let out = extract_catalog_signals(
        &doc,
        None,
        1,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["title"], "Launch Filespec");
    assert_eq!(value[0]["url"], "https://example.com/docs/uf.pdf#Intro");
    assert_eq!(value[0]["dest"], serde_json::Value::Null);
}

#[test]
fn page_labels_kids_number_tree_matches_pdfjs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-page-labels-kids-v1.pdf");
    if !path.is_file() {
        return;
    }
    let document = Document::load(path).expect("load kids labels fixture");
    let out = extract_catalog_signals(
        &document,
        None,
        3,
        CatalogSignalRequest {
            page_labels: true,
            permissions: false,
            outline: false,
        },
    );
    assert_eq!(
        out.page_labels,
        Some(vec!["i".to_string(), "ii".to_string(), "10".to_string()])
    );
}

#[test]
fn invalid_page_label_dictionary_omits_the_whole_surface() {
    let doc = document(dictionary! {
        "PageLabels"=>dictionary!{"Nums"=>vec![
            0.into(),
            Object::Dictionary(dictionary!{"S"=>"Bogus"}),
        ]},
    });
    let out = extract_catalog_signals(
        &doc,
        None,
        2,
        CatalogSignalRequest {
            page_labels: true,
            permissions: false,
            outline: false,
        },
    );
    assert!(out.page_labels.is_none());
}

#[test]
fn decoded_text_expansion_sticky_exhausts_the_source_budget() {
    let doc = document(dictionary! {});
    let mut walker = Walker::new(&doc);
    walker.text_remaining = 1;
    assert!(walker.text(&Object::string_literal(vec![0x80])).is_none());
    assert_eq!(walker.text_remaining, 0);
    assert!(walker.text(&Object::string_literal("x")).is_none());
}

#[test]
fn unsafe_outline_uri_is_omitted_and_destination_remains_null() {
    let mut doc = document(dictionary! {});
    let first = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("Unsafe"),
        "A"=>dictionary!{"S"=>"URI","URI"=>Object::string_literal("javascript:alert(1)")},
    });
    let outlines = doc.add_object(dictionary! {"First"=>first});
    doc.catalog_mut().unwrap().set("Outlines", outlines);
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert!(value[0].get("url").is_none());
    assert_eq!(value[0]["dest"], serde_json::Value::Null);
}

#[test]
fn outline_cycle_is_suppressed_by_pdfjs_global_processed_set() {
    let mut doc = document(dictionary! {});
    let first = doc.add_object(dictionary! {"Title"=>Object::string_literal("Cycle")});
    doc.get_object_mut(first)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Next", first);
    let outlines = doc.add_object(dictionary! {"First"=>first});
    doc.catalog_mut().unwrap().set("Outlines", outlines);
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 1);
    assert_eq!(value[0]["title"], "Cycle");
}

#[test]
fn page_label_overflow_omits_only_that_surface() {
    let mut doc = document(dictionary! {});
    let excessive_kids = (0..=MAX_ENTRIES)
        .map(|_| Object::Dictionary(Dictionary::new()))
        .collect::<Vec<_>>();
    let labels = doc.add_object(dictionary! {"Kids"=>excessive_kids});
    doc.catalog_mut().unwrap().set("PageLabels", labels);

    let first = doc.add_object(dictionary! {"Title"=>Object::string_literal("Still valid")});
    let outlines = doc.add_object(dictionary! {"First"=>first});
    doc.catalog_mut().unwrap().set("Outlines", outlines);

    let out = extract_catalog_signals(
        &doc,
        None,
        1,
        CatalogSignalRequest {
            page_labels: true,
            permissions: false,
            outline: true,
        },
    );
    assert!(out.page_labels.is_none());
    assert_eq!(out.outline.unwrap().len(), 1);
}

#[test]
fn mark_info_reference_cycle_omits_instead_of_inventing_false() {
    let mut doc = document(dictionary! {});
    let cycle = doc.add_object(Object::Null);
    *doc.get_object_mut(cycle).unwrap() = Object::Reference(cycle);
    doc.catalog_mut()
        .unwrap()
        .set("MarkInfo", dictionary! {"Marked"=>cycle});
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: true,
            outline: false,
        },
    );
    assert!(out.mark_info.is_none());
}

#[test]
fn indirect_number_tree_array_is_resolved() {
    let mut doc = document(dictionary! {});
    let nums = doc.add_object(Object::Array(vec![
        0.into(),
        Object::Dictionary(dictionary! {"S"=>"D"}),
    ]));
    let labels = doc.add_object(dictionary! {"Nums"=>nums});
    doc.catalog_mut().unwrap().set("PageLabels", labels);
    let out = extract_catalog_signals(
        &doc,
        None,
        2,
        CatalogSignalRequest {
            page_labels: true,
            permissions: false,
            outline: false,
        },
    );
    assert_eq!(out.page_labels, Some(vec!["1".into(), "2".into()]));
}

#[test]
fn direct_outline_link_is_omitted_like_pdfjs() {
    let doc = document(dictionary! {
        "Outlines"=>dictionary!{"First"=>dictionary!{"Title"=>Object::string_literal("Direct")}},
    });
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    assert!(out.outline.is_none());
}

#[test]
fn url_normalization_expansion_respects_the_field_cap() {
    let mut doc = document(dictionary! {});
    let oversized_after_normalization = format!("https://example.com/{}", "\"".repeat(30_000));
    let first = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("Expanded"),
        "A"=>dictionary!{
            "S"=>"URI",
            "URI"=>Object::string_literal(oversized_after_normalization),
        },
    });
    let outlines = doc.add_object(dictionary! {"First"=>first});
    doc.catalog_mut().unwrap().set("Outlines", outlines);
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    assert!(out.outline.is_none());
}

#[test]
fn duplicate_number_tree_reference_omits_the_whole_surface() {
    let mut doc = document(dictionary! {});
    let leaf = doc.add_object(dictionary! {
        "Nums"=>vec![0.into(),Object::Dictionary(dictionary!{"S"=>"D"})],
    });
    let labels = doc.add_object(dictionary! {"Kids"=>vec![leaf.into(),leaf.into()]});
    doc.catalog_mut().unwrap().set("PageLabels", labels);
    let out = extract_catalog_signals(
        &doc,
        None,
        1,
        CatalogSignalRequest {
            page_labels: true,
            permissions: false,
            outline: false,
        },
    );
    assert!(out.page_labels.is_none());
}

#[test]
fn number_tree_kids_take_precedence_over_same_node_nums() {
    let mut doc = document(dictionary! {});
    let leaf = doc.add_object(dictionary! {
        "Nums"=>vec![0.into(),Object::Dictionary(dictionary!{"S"=>"D"})],
    });
    let labels = doc.add_object(dictionary! {
        "Kids"=>vec![leaf.into()],
        "Nums"=>vec![0.into(),Object::Dictionary(dictionary!{"P"=>Object::string_literal("wrong")})],
    });
    doc.catalog_mut().unwrap().set("PageLabels", labels);
    let out = extract_catalog_signals(
        &doc,
        None,
        2,
        CatalogSignalRequest {
            page_labels: true,
            permissions: false,
            outline: false,
        },
    );
    assert_eq!(out.page_labels, Some(vec!["1".into(), "2".into()]));
}

#[test]
fn shared_outline_reference_is_emitted_only_on_first_pdfjs_branch() {
    let mut doc = document(dictionary! {});
    let shared = doc.add_object(dictionary! {"Title"=>Object::string_literal("Shared")});
    let second = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("Second"),
        "First"=>shared,
    });
    let first = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("First"),
        "First"=>shared,
        "Next"=>second,
    });
    let outlines = doc.add_object(dictionary! {"First"=>first});
    doc.catalog_mut().unwrap().set("Outlines", outlines);
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["items"][0]["title"], "Shared");
    assert!(value[1].get("items").is_none());
}

#[test]
fn outline_first_and_next_are_globally_admitted_before_child_runs() {
    let mut doc = document(dictionary! {});
    let shared = doc.add_object(dictionary! {"Title"=>Object::string_literal("Shared")});
    let child = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("Child"),
        "First"=>shared,
    });
    let first = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("First"),
        "First"=>child,
        "Next"=>shared,
    });
    let outlines = doc.add_object(dictionary! {"First"=>first});
    doc.catalog_mut().unwrap().set("Outlines", outlines);
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["items"][0]["title"], "Child");
    assert!(value[0]["items"][0].get("items").is_none());
    assert_eq!(value[1]["title"], "Shared");
}

#[test]
fn outline_fifo_assigns_deep_shared_ref_to_earlier_queued_sibling() {
    let mut doc = document(dictionary! {});
    let z = doc.add_object(dictionary! {"Title"=>Object::string_literal("Z")});
    let x = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("X"),
        "First"=>z,
    });
    let b = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("B"),
        "First"=>z,
    });
    let c = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("C"),
        "First"=>x,
    });
    let a = doc.add_object(dictionary! {
        "Title"=>Object::string_literal("A"),
        "First"=>c,
        "Next"=>b,
    });
    let outlines = doc.add_object(dictionary! {"First"=>a});
    doc.catalog_mut().unwrap().set("Outlines", outlines);
    let out = extract_catalog_signals(
        &doc,
        None,
        0,
        CatalogSignalRequest {
            page_labels: false,
            permissions: false,
            outline: true,
        },
    );
    let value = serde_json::to_value(out.outline).unwrap();
    assert_eq!(value[0]["title"], "A");
    assert_eq!(value[0]["items"][0]["title"], "C");
    assert_eq!(value[0]["items"][0]["items"][0]["title"], "X");
    assert!(value[0]["items"][0]["items"][0].get("items").is_none());
    assert_eq!(value[1]["title"], "B");
    assert_eq!(value[1]["items"][0]["title"], "Z");
}
