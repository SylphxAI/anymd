use super::*;
use lopdf::{dictionary, Stream};

fn base_document() -> (Document, ObjectId) {
    let mut document = Document::with_version("1.7");
    let pages =
        document.add_object(dictionary! {"Type"=>"Pages","Kids"=>Vec::<Object>::new(),"Count"=>0});
    (document, pages)
}

fn finish_catalog(document: &mut Document, pages: ObjectId, extra: Dictionary) {
    let mut catalog = dictionary! {"Type"=>"Catalog","Pages"=>pages};
    catalog.extend(&extra);
    let root = document.add_object(catalog);
    document.trailer.set("Root", root);
}
#[test]
fn path_filename_is_stripped_and_unfiltered_size_is_actual() {
    let mut doc = Document::with_version("1.7");
    let pages =
        doc.add_object(dictionary! {"Type"=>"Pages","Kids"=>Vec::<Object>::new(),"Count"=>0});
    let stream = doc.add_object(Stream::new(dictionary! {}, b"hello".to_vec()));
    let spec=doc.add_object(dictionary!{"UF"=>Object::string_literal(r"C:\reports\a.txt"),"EF"=>dictionary!{"UF"=>stream}});
    let tree =
        doc.add_object(dictionary! {"Names"=>vec![Object::string_literal("key"),spec.into()]});
    let root = doc.add_object(
        dictionary! {"Type"=>"Catalog","Pages"=>pages,"Names"=>dictionary!{"EmbeddedFiles"=>tree}},
    );
    doc.trailer.set("Root", root);
    let out = extract_form_attachment_signals(&doc, &[], false, true);
    assert_eq!(
        serde_json::to_value(out.attachments).unwrap(),
        serde_json::json!([{"name":"key","filename":"a.txt","size_bytes":5}])
    );
}

#[test]
fn frozen_v3014_forms_and_attachments_match_the_real_ts_subset() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/differential/v3014-behavior-v1.pdf");
    let document = Document::load(path).expect("load immutable behavior fixture");
    let pages = document.get_pages().into_iter().collect::<Vec<_>>();
    let output = extract_form_attachment_signals(&document, &pages, true, true);
    assert!(output.warnings.is_empty());
    assert_eq!(
        serde_json::to_value(output.form_fields).unwrap(),
        serde_json::json!([
            {"name":"customer_name","type":"text","value":"Ada Lovelace","default_value":"","page":1,"id":"22R","editable":true,"bounding_box":{"left":72.0,"bottom":635.0,"right":260.0,"top":660.0}},
            {"name":"profile","id":"24R"},
            {"name":"profile","type":"text","value":"Grace Hopper","default_value":"Unknown","page":2,"id":"25R","editable":false,"bounding_box":{"left":72.0,"bottom":500.0,"right":260.0,"top":525.0}},
            {"name":"consent","type":"checkbox","value":"Yes","default_value":null,"page":2,"id":"26R","editable":true,"bounding_box":{"left":72.0,"bottom":450.0,"right":90.0,"top":468.0}},
            {"name":"tier","type":"listbox","value":"gold","default_value":"silver","page":3,"id":"27R","editable":true,"bounding_box":{"left":72.0,"bottom":400.0,"right":200.0,"top":425.0}}
        ])
    );
    assert_eq!(
        serde_json::to_value(output.attachments).unwrap(),
        serde_json::json!([
            {"name":"source.csv","filename":"source.csv","description":"Source data","size_bytes":19},
            {"name":"evidence","filename":"report.txt","size_bytes":5}
        ])
    );
}

#[test]
fn forms_skip_direct_top_level_and_direct_kids_and_use_dv_as_current_value() {
    let (mut document, pages) = base_document();
    let child = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("  child  "),
        "DV"=>Object::string_literal("fallback")
    });
    let parent = document.add_object(dictionary! {
        "T"=>Object::string_literal("  parent  "),
        "Kids"=>vec![Object::Dictionary(dictionary!{"Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("direct")}),child.into()]
    });
    let direct = Object::Dictionary(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("top-direct")
    });
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"AcroForm"=>dictionary!{"Fields"=>vec![direct,parent.into()]}},
    );
    let output = extract_form_attachment_signals(&document, &[], true, false);
    assert_eq!(
        serde_json::to_value(output.form_fields).unwrap(),
        serde_json::json!([
            {"name":"parent","id":format_id(parent)},
            {"name":"child","type":"text","value":"fallback","default_value":"fallback","id":format_id(child),"editable":true}
        ])
    );
}

#[test]
fn name_tree_kids_win_over_names_and_wrong_type_kids_skip_the_node() {
    let (mut document, pages) = base_document();
    let stream = document.add_object(Stream::new(dictionary! {}, b"x".to_vec()));
    let spec = document.add_object(
        dictionary! {"F"=>Object::string_literal("a.txt"),"EF"=>dictionary!{"F"=>stream}},
    );
    let child = document
        .add_object(dictionary! {"Names"=>vec![Object::string_literal("child"),spec.into()]});
    let tree = document.add_object(dictionary! {
        "Kids"=>vec![child.into()],
        "Names"=>vec![Object::string_literal("must-skip"),spec.into()]
    });
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"Names"=>dictionary!{"EmbeddedFiles"=>tree}},
    );
    let output = extract_form_attachment_signals(&document, &[], false, true);
    assert_eq!(
        serde_json::to_value(output.attachments).unwrap(),
        serde_json::json!([{"name":"child","filename":"a.txt","size_bytes":1}])
    );

    let (mut document, pages) = base_document();
    let tree = document.add_object(
        dictionary! {"Kids"=>7,"Names"=>vec![Object::string_literal("ignored"),Object::Null]},
    );
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"Names"=>dictionary!{"EmbeddedFiles"=>tree}},
    );
    assert!(extract_form_attachment_signals(&document, &[], false, true)
        .attachments
        .is_none());
}

#[test]
fn indirect_name_tree_arrays_work_duplicate_kids_fail_and_invalid_uf_does_not_fallback() {
    let (mut document, pages) = base_document();
    let stream = document.add_object(Stream::new(dictionary! {}, b"xy".to_vec()));
    let spec=document.add_object(dictionary!{"UF"=>7,"F"=>Object::string_literal("fallback.txt"),"EF"=>dictionary!{"F"=>stream}});
    let names_array = document.add_object(vec![Object::string_literal("key"), spec.into()]);
    let child = document.add_object(dictionary! {"Names"=>names_array});
    let kids_array = document.add_object(vec![child.into()]);
    let tree = document.add_object(dictionary! {"Kids"=>kids_array});
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"Names"=>dictionary!{"EmbeddedFiles"=>tree}},
    );
    let output = extract_form_attachment_signals(&document, &[], false, true);
    assert_eq!(
        serde_json::to_value(output.attachments).unwrap(),
        serde_json::json!([{"name":"key","filename":"unnamed","size_bytes":2}])
    );

    let (mut document, pages) = base_document();
    let child = document.add_object(dictionary! {});
    let tree = document.add_object(dictionary! {"Kids"=>vec![child.into(),child.into()]});
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"Names"=>dictionary!{"EmbeddedFiles"=>tree}},
    );
    assert!(extract_form_attachment_signals(&document, &[], false, true)
        .attachments
        .is_none());
}

#[test]
fn broken_top_level_reference_fails_the_whole_form_surface() {
    let (mut document, pages) = base_document();
    let valid = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("valid")
    });
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"AcroForm"=>dictionary!{"Fields"=>vec![valid.into(),Object::Reference((999,0))]}},
    );
    assert!(extract_form_attachment_signals(&document, &[], true, false)
        .form_fields
        .is_none());
}

#[test]
fn form_values_follow_pdfjs_decode_and_widget_coercion() {
    let (mut document, pages) = base_document();
    let text_numeric = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("numeric"),"V"=>7,"DV"=>Object::string_literal("default")
    });
    let text_missing = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("missing")
    });
    let button_bool = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Btn","T"=>Object::string_literal("button"),"V"=>true,"DV"=>Object::Name(b"Default".to_vec())
    });
    let choice_filtered = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Ch","T"=>Object::string_literal("choice"),
        "V"=>vec![7.into(),Object::string_literal("first"),Object::Null,Object::string_literal("second")]
    });
    let choice_fallback = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Ch","T"=>Object::string_literal("fallback"),
        "DV"=>vec![false.into(),Object::string_literal("from-default")]
    });
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"AcroForm"=>dictionary!{"Fields"=>vec![text_numeric.into(),text_missing.into(),button_bool.into(),choice_filtered.into(),choice_fallback.into()]}},
    );
    let output = extract_form_attachment_signals(&document, &[], true, false);
    assert_eq!(
        serde_json::to_value(output.form_fields).unwrap(),
        serde_json::json!([
            {"name":"numeric","type":"text","value":"","default_value":"default","id":format_id(text_numeric),"editable":true},
            {"name":"missing","type":"text","value":"","default_value":"","id":format_id(text_missing),"editable":true},
            {"name":"button","type":"checkbox","value":"Off","default_value":"Default","id":format_id(button_bool),"editable":true},
            {"name":"choice","type":"listbox","value":"first","default_value":null,"id":format_id(choice_filtered),"editable":true},
            {"name":"fallback","type":"listbox","value":"from-default","default_value":["from-default"],"id":format_id(choice_fallback),"editable":true}
        ])
    );
}

#[test]
fn button_array_v_and_dv_preserve_pdfjs_arrays() {
    let mut document = Document::with_version("1.4");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! {
        "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()]
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type"=>"Pages","Kids"=>vec![page_id.into()],"Count"=>1
        }),
    );
    let checkbox_array = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Btn","T"=>Object::string_literal("flags"),
        "V"=>vec![Object::Name(b"Yes".to_vec()), Object::Name(b"Off".to_vec())],
        "Rect"=>vec![72.into(),650.into(),100.into(),670.into()],"P"=>page_id
    });
    let radio_array = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Btn","T"=>Object::string_literal("tier"),
        "V"=>vec![Object::Name(b"Gold".to_vec()), Object::Name(b"Silver".to_vec())],
        "Ff"=>1i64<<15,
        "Rect"=>vec![72.into(),620.into(),100.into(),640.into()],"P"=>page_id
    });
    let push_array = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Btn","T"=>Object::string_literal("go"),
        "V"=>vec![Object::Name(b"Off".to_vec()), Object::Name(b"Pushed".to_vec())],
        "Ff"=>1i64<<16,
        "Rect"=>vec![72.into(),590.into(),140.into(),610.into()],"P"=>page_id
    });
    let plain = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Btn","T"=>Object::string_literal("plain"),
        "V"=>Object::Name(b"Yes".to_vec()),
        "Rect"=>vec![72.into(),560.into(),100.into(),580.into()],"P"=>page_id
    });
    let empty_array = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Btn","T"=>Object::string_literal("empty"),
        "V"=>Vec::<Object>::new(),
        "Rect"=>vec![72.into(),530.into(),100.into(),550.into()],"P"=>page_id
    });
    let dv_array = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Btn","T"=>Object::string_literal("dv"),
        "DV"=>vec![Object::Name(b"Yes".to_vec()), Object::Name(b"Off".to_vec())],
        "Rect"=>vec![72.into(),500.into(),100.into(),520.into()],"P"=>page_id
    });
    let catalog = document.add_object(dictionary! {
        "Type"=>"Catalog","Pages"=>pages_id,
        "AcroForm"=>dictionary! {
            "Fields"=>vec![
                checkbox_array.into(), radio_array.into(), push_array.into(),
                plain.into(), empty_array.into(), dv_array.into()
            ]
        }
    });
    document.trailer.set("Root", catalog);
    let signals = extract_form_attachment_signals(&document, &[(1, page_id)], true, false);
    let fields = signals.form_fields.expect("fields");
    let value_of = |name: &str| {
        fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| serde_json::to_value(&field.value).unwrap())
            .expect(name)
    };
    let type_of = |name: &str| {
        fields
            .iter()
            .find(|field| field.name == name)
            .and_then(|field| field.r#type.clone())
            .expect(name)
    };
    let default_of = |name: &str| {
        fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| serde_json::to_value(&field.default_value).unwrap())
            .expect(name)
    };
    assert_eq!(value_of("flags"), serde_json::json!(["Yes", "Off"]));
    assert_eq!(value_of("tier"), serde_json::json!(["Gold", "Silver"]));
    assert_eq!(type_of("tier"), "radiobutton");
    assert_eq!(value_of("go"), serde_json::json!(["Off", "Pushed"]));
    assert_eq!(type_of("go"), "button");
    assert_eq!(value_of("plain"), serde_json::json!("Yes"));
    assert_eq!(value_of("empty"), serde_json::json!("Off"));
    assert_eq!(value_of("dv"), serde_json::json!(["Yes", "Off"]));
    assert_eq!(default_of("dv"), serde_json::json!(["Yes", "Off"]));
}

#[test]
fn utf16_be_odd_length_form_values_drop_trailing_byte_like_pdfjs() {
    let mut document = Document::with_version("1.4");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! {
        "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()]
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type"=>"Pages","Kids"=>vec![page_id.into()],"Count"=>1
        }),
    );
    // FEFF 0041 0064 0061 => "Ada"
    let valid = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("valid"),
        "V"=>Object::String(vec![0xFE,0xFF,0x00,0x41,0x00,0x64,0x00,0x61], lopdf::StringFormat::Hexadecimal),
        "Rect"=>vec![72.into(),650.into(),200.into(),670.into()],"P"=>page_id
    });
    // FEFF 0042 006F 62 => drop trailing 0x62 => "Bo"
    let odd = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("odd"),
        "V"=>Object::String(vec![0xFE,0xFF,0x00,0x42,0x00,0x6F,0x62], lopdf::StringFormat::Hexadecimal),
        "DV"=>Object::String(vec![0xFE,0xFF,0x00,0x42,0x00,0x6F,0x62], lopdf::StringFormat::Hexadecimal),
        "Rect"=>vec![72.into(),620.into(),200.into(),640.into()],"P"=>page_id
    });
    // plain PDFDocEncoding control
    let plain = document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("plain"),
        "V"=>Object::string_literal("Alice"),
        "Rect"=>vec![72.into(),590.into(),200.into(),610.into()],"P"=>page_id
    });
    let catalog = document.add_object(dictionary! {
        "Type"=>"Catalog","Pages"=>pages_id,
        "AcroForm"=>dictionary!{"Fields"=>vec![valid.into(), odd.into(), plain.into()]}
    });
    document.trailer.set("Root", catalog);
    let signals = extract_form_attachment_signals(&document, &[(1, page_id)], true, false);
    let fields = signals.form_fields.expect("fields");
    let value_of = |name: &str| {
        fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| serde_json::to_value(&field.value).unwrap())
            .expect(name)
    };
    let default_of = |name: &str| {
        fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| serde_json::to_value(&field.default_value).unwrap())
            .expect(name)
    };
    assert_eq!(value_of("valid"), serde_json::json!("Ada"));
    assert_eq!(value_of("odd"), serde_json::json!("Bo"));
    assert_eq!(default_of("odd"), serde_json::json!("Bo"));
    assert_eq!(value_of("plain"), serde_json::json!("Alice"));
}

#[test]
fn odd_length_names_materialize_trailing_orphan_like_pdfjs() {
    let (mut document, pages) = base_document();
    let stream = document.add_object(Stream::new(dictionary! {}, b"x".to_vec()));
    let spec = document.add_object(dictionary! {
        "F"=>Object::string_literal("a.txt"),
        "EF"=>dictionary!{"F"=>stream}
    });
    let tree = document.add_object(dictionary! {
        "Names"=>vec![
            Object::string_literal("child"),
            spec.into(),
            Object::string_literal("orphan"),
        ]
    });
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"Names"=>dictionary!{"EmbeddedFiles"=>tree}},
    );
    let out = extract_form_attachment_signals(&document, &[], false, true);
    assert!(out.warnings.is_empty());
    assert_eq!(
        serde_json::to_value(out.attachments).unwrap(),
        serde_json::json!([
            {"name":"child","filename":"a.txt","size_bytes":1},
            {"name":"orphan","filename":"unnamed"}
        ])
    );
}

#[test]
fn orphan_only_names_array_materializes_unnamed_attachment() {
    let (mut document, pages) = base_document();
    let tree = document.add_object(dictionary! {
        "Names"=>vec![Object::string_literal("orphan")]
    });
    finish_catalog(
        &mut document,
        pages,
        dictionary! {"Names"=>dictionary!{"EmbeddedFiles"=>tree}},
    );
    let out = extract_form_attachment_signals(&document, &[], false, true);
    assert!(out.warnings.is_empty());
    assert_eq!(
        serde_json::to_value(out.attachments).unwrap(),
        serde_json::json!([{"name":"orphan","filename":"unnamed"}])
    );
}

#[test]
fn trailing_slash_filename_becomes_unnamed() {
    let document = Document::with_version("1.7");
    let mut walker = Walker::new(&document);
    let spec = dictionary! {"UF"=>Object::string_literal(r"folder/")};
    assert_eq!(filename(&mut walker, &spec), Some("unnamed".into()));
}

fn valid_attachment_tree(document: &mut Document) -> ObjectId {
    let stream = document.add_object(Stream::new(dictionary! {}, b"x".to_vec()));
    let spec = document.add_object(
        dictionary! {"F"=>Object::string_literal("ok.txt"),"EF"=>dictionary!{"F"=>stream}},
    );
    document.add_object(dictionary! {"Names"=>vec![Object::string_literal("ok"),spec.into()]})
}

fn valid_form(document: &mut Document) -> ObjectId {
    document.add_object(dictionary! {
        "Subtype"=>"Widget","FT"=>"Tx","T"=>Object::string_literal("ok")
    })
}

#[test]
fn root_fields_and_form_kids_have_global_raw_admission_budgets() {
    for oversized_kids in [false, true] {
        let (mut document, pages) = base_document();
        let attachment_tree = valid_attachment_tree(&mut document);
        let fields = if oversized_kids {
            let parent = document.add_object(dictionary! {
                "T"=>Object::string_literal("parent"),
                "Kids"=>vec![Object::Null;MAX_ENTRIES+1]
            });
            vec![parent.into()]
        } else {
            vec![Object::Null; MAX_ENTRIES + 1]
        };
        let acroform = document.add_object(dictionary! {"Fields"=>fields});
        finish_catalog(
            &mut document,
            pages,
            dictionary! {
                "AcroForm"=>acroform,
                "Names"=>dictionary!{"EmbeddedFiles"=>attachment_tree}
            },
        );
        let output = extract_form_attachment_signals(&document, &[], true, true);
        assert!(output.form_fields.is_none());
        assert!(output.attachments.is_some());
        assert!(output
            .warnings
            .iter()
            .any(|warning| warning.contains("include_form_fields")));
        assert_eq!(
            output.form_materialized_array_items,
            usize::from(oversized_kids),
            "an oversized direct array must not materialize its children"
        );
    }
}

#[test]
fn name_tree_kids_and_pairs_have_global_admission_budgets() {
    for oversized_pairs in [false, true] {
        let (mut document, pages) = base_document();
        let form = valid_form(&mut document);
        let tree = if oversized_pairs {
            document.add_object(dictionary! {
                "Names"=>vec![Object::Null; (MAX_ENTRIES+1)*2]
            })
        } else {
            document.add_object(dictionary! {
                "Kids"=>vec![Object::Null;MAX_ENTRIES+1]
            })
        };
        finish_catalog(
            &mut document,
            pages,
            dictionary! {
                "AcroForm"=>dictionary!{"Fields"=>vec![form.into()]},
                "Names"=>dictionary!{"EmbeddedFiles"=>tree}
            },
        );
        let output = extract_form_attachment_signals(&document, &[], true, true);
        assert!(output.form_fields.is_some());
        assert!(output.attachments.is_none());
        assert!(output
            .warnings
            .iter()
            .any(|warning| warning.contains("include_attachments")));
        assert_eq!(
            output.attachment_materialized_array_items, 0,
            "an oversized NameTree collection must fail before item materialization"
        );
    }
}

#[test]
fn page_and_annotation_admission_is_bounded_before_form_maps() {
    for indirect_annots in [false, true] {
        let (mut document, pages_root) = base_document();
        let form = valid_form(&mut document);
        let attachment_tree = valid_attachment_tree(&mut document);
        let annots = vec![Object::Null; MAX_ENTRIES + 1];
        let page = if indirect_annots {
            let annots = document.add_object(annots);
            document.add_object(dictionary! {"Type"=>"Page","Annots"=>annots})
        } else {
            document.add_object(dictionary! {"Type"=>"Page","Annots"=>annots})
        };
        finish_catalog(
            &mut document,
            pages_root,
            dictionary! {
                "AcroForm"=>dictionary!{"Fields"=>vec![form.into()]},
                "Names"=>dictionary!{"EmbeddedFiles"=>attachment_tree}
            },
        );
        let output = extract_form_attachment_signals(&document, &[(1, page)], true, true);
        assert!(output.form_fields.is_none());
        assert!(output.attachments.is_some());
        assert!(output
            .warnings
            .iter()
            .any(|warning| warning.contains("include_form_fields")));
        assert_eq!(output.form_annotation_materialized_array_items, 0);
    }

    let (mut document, pages_root) = base_document();
    let form = valid_form(&mut document);
    let attachment_tree = valid_attachment_tree(&mut document);
    let page = document.add_object(dictionary! {"Type"=>"Page"});
    finish_catalog(
        &mut document,
        pages_root,
        dictionary! {
            "AcroForm"=>dictionary!{"Fields"=>vec![form.into()]},
            "Names"=>dictionary!{"EmbeddedFiles"=>attachment_tree}
        },
    );
    let pages = vec![(1, page); MAX_ENTRIES + 1];
    let output = extract_form_attachment_signals(&document, &pages, true, true);
    assert!(output.form_fields.is_none());
    assert!(output.attachments.is_some());
    assert_eq!(output.form_annotation_materialized_array_items, 0);
}

#[test]
fn form_value_depth_and_aggregate_nodes_are_bounded() {
    let mut deep = Object::Name(b"leaf".to_vec());
    for _ in 0..=MAX_DEPTH {
        deep = Object::Array(vec![deep]);
    }
    let child = Object::Array(vec![Object::Null; 256]);
    let aggregate = Object::Array(vec![child; (MAX_ENTRIES / 256) + 2]);
    let oversized = Object::Array(vec![Object::Null; 257]);
    for value in [deep, aggregate, oversized] {
        let (mut document, pages) = base_document();
        let field = document.add_object(dictionary! {
            "Subtype"=>"Widget","FT"=>"Ch","T"=>Object::string_literal("hostile"),
            "V"=>value
        });
        let attachment_tree = valid_attachment_tree(&mut document);
        finish_catalog(
            &mut document,
            pages,
            dictionary! {
                "AcroForm"=>dictionary!{"Fields"=>vec![field.into()]},
                "Names"=>dictionary!{"EmbeddedFiles"=>attachment_tree}
            },
        );
        let output = extract_form_attachment_signals(&document, &[], true, true);
        assert!(output.form_fields.is_none());
        assert!(output.attachments.is_some());
        assert!(output
            .warnings
            .iter()
            .any(|warning| warning.contains("include_form_fields")));
    }
}

#[test]
fn bounded_array_resolution_rejects_reference_depth_overflow() {
    let mut document = Document::with_version("1.7");
    let mut id = document.add_object(Vec::<Object>::new());
    for _ in 0..=MAX_DEPTH {
        id = document.add_object(Object::Reference(id));
    }
    document.trailer.set("Root", id);
    let root = document.trailer.get(b"Root").unwrap();
    let mut walker = Walker::new(&document);
    assert!(walker.array_bounded(root, MAX_ENTRIES).is_none());
    assert!(walker.limited);
    assert!(!walker.failed);
}

#[test]
fn button_ap_default_off_when_named_normal_appearance() {
    let cases = [
        (
            "v3014-form-checkbox-ap-default-off-v1.pdf",
            "Agree",
            "checkbox",
            "Yes",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-radio-ap-default-off-v1.pdf",
            "Plan",
            "radiobutton",
            "Gold",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-noap-default-null-v1.pdf",
            "Consent",
            "checkbox",
            "Yes",
            serde_json::Value::Null,
        ),
    ];
    for (fixture, name, kind, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load button default fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, name, "fixture {fixture}");
        assert_eq!(field.r#type.as_deref(), Some(kind), "fixture {fixture}");
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}

#[test]
fn pushbutton_ap_default_stays_null() {
    let cases = [
        (
            "v3014-form-pushbutton-ap-default-null-v1.pdf",
            "Go",
            "button",
            "Off",
            serde_json::Value::Null,
        ),
        (
            "v3014-form-pushbutton-noap-default-null-v1.pdf",
            "Go",
            "button",
            "Off",
            serde_json::Value::Null,
        ),
        (
            "v3014-form-checkbox-ap-default-off-v1.pdf",
            "Agree",
            "checkbox",
            "Yes",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, name, kind, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load pushbutton default fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, name, "fixture {fixture}");
        assert_eq!(field.r#type.as_deref(), Some(kind), "fixture {fixture}");
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}

#[test]
fn checkbox_as_overrides_v_when_named_normal_appearance() {
    let cases = [
        (
            "v3014-form-checkbox-as-overrides-v-off-v1.pdf",
            "Off",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-as-overrides-v-yes-v1.pdf",
            "Yes",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-as-noap-keeps-v-v1.pdf",
            "Yes",
            serde_json::Value::Null,
        ),
    ];
    for (fixture, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load checkbox AS fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, "Agree", "fixture {fixture}");
        assert_eq!(
            field.r#type.as_deref(),
            Some("checkbox"),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}

#[test]
fn checkbox_export_value_normalization() {
    let cases = [
        (
            "v3014-form-checkbox-as-invalid-export-off-v1.pdf",
            "Off",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-as-only-off-yes-v1.pdf",
            "Yes",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-as-off-export-ok-v1.pdf",
            "Off",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load checkbox export fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, "Agree", "fixture {fixture}");
        assert_eq!(
            field.r#type.as_deref(),
            Some("checkbox"),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}
#[test]
fn radio_as_does_not_override_v() {
    let cases = [
        (
            "v3014-form-radio-as-does-not-override-v-v1.pdf",
            "radiobutton",
            "Gold",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-radio-as-invalid-keeps-v-v1.pdf",
            "radiobutton",
            "Gold",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-radio-checkbox-as-regression-v1.pdf",
            "checkbox",
            "Off",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, field_type, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load radio fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(
            field.r#type.as_deref(),
            Some(field_type),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}
#[test]
fn checkbox_multi_export_options() {
    let cases = [
        (
            "v3014-form-checkbox-multi-export-foo-v1.pdf",
            "Foo",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-multi-export-as-bar-v1.pdf",
            "Bar",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-multi-export-as-baz-off-v1.pdf",
            "Off",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load multi-export fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, "Agree", "fixture {fixture}");
        assert_eq!(
            field.r#type.as_deref(),
            Some("checkbox"),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}
#[test]
fn checkbox_multi_export_many_options() {
    let cases = [
        (
            "v3014-form-checkbox-multi-export-many-c-v1.pdf",
            "C",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-multi-export-many-a-v1.pdf",
            "A",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-multi-export-many-z-off-v1.pdf",
            "Off",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load multi-export-many fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, "Agree", "fixture {fixture}");
        assert_eq!(
            field.r#type.as_deref(),
            Some("checkbox"),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}
#[test]
fn checkbox_export_empty_single_options() {
    let cases = [
        (
            "v3014-form-checkbox-export-empty-ap-yes-v1.pdf",
            "Yes",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-export-single-foo-v1.pdf",
            "Foo",
            serde_json::json!("Off"),
        ),
        (
            "v3014-form-checkbox-export-single-bar-off-v1.pdf",
            "Off",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load empty/single export fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, "Agree", "fixture {fixture}");
        assert_eq!(
            field.r#type.as_deref(),
            Some("checkbox"),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}
#[test]
fn checkbox_malformed_ap_keeps_v() {
    let cases = [
        (
            "v3014-form-checkbox-ap-stream-keeps-v-v1.pdf",
            "Yes",
            serde_json::Value::Null,
        ),
        (
            "v3014-form-checkbox-apn-stream-keeps-v-v1.pdf",
            "Yes",
            serde_json::Value::Null,
        ),
        (
            "v3014-form-checkbox-ap-named-as-off-v1.pdf",
            "Off",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load malformed-ap fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, "Agree", "fixture {fixture}");
        assert_eq!(
            field.r#type.as_deref(),
            Some("checkbox"),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}
#[test]
fn radio_malformed_ap_keeps_v() {
    let cases = [
        (
            "v3014-form-radio-ap-stream-keeps-v-v1.pdf",
            "Gold",
            serde_json::Value::Null,
        ),
        (
            "v3014-form-radio-apn-stream-keeps-v-v1.pdf",
            "Gold",
            serde_json::Value::Null,
        ),
        (
            "v3014-form-radio-ap-named-keeps-v-v1.pdf",
            "Gold",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load radio malformed-ap fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 1, "fixture {fixture}");
        let field = &fields[0];
        assert_eq!(field.name, "Plan", "fixture {fixture}");
        assert_eq!(
            field.r#type.as_deref(),
            Some("radiobutton"),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.value).unwrap(),
            serde_json::json!(value),
            "fixture {fixture}"
        );
        assert_eq!(
            serde_json::to_value(&field.default_value).unwrap(),
            default_value,
            "fixture {fixture}"
        );
    }
}

#[test]
fn radio_broken_parent_chain_drops_widgets_and_names_intermediate_opt() {
    // pdf.js: intermediate without Parent does not inherit FT/V from radio root.
    // Public names use Parent-chain construction, so intermediate is "Opt" not "Plan.Opt".
    let mut document = Document::with_version("1.4");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! {
        "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()]
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type"=>"Pages","Kids"=>vec![page_id.into()],"Count"=>1
        }),
    );
    let gold = document.add_object(Stream::new(dictionary! {}, Vec::new()));
    let off = document.add_object(Stream::new(dictionary! {}, Vec::new()));
    let intermediate = document.add_object(dictionary! {
        "T"=>Object::string_literal("Opt"),
        "Kids"=>Vec::<Object>::new()
    });
    let leaf0 = document.add_object(dictionary! {
        "Type"=>"Annot","Subtype"=>"Widget","Parent"=>intermediate,
        "AS"=>"Silver",
        "Rect"=>vec![72.into(),600.into(),100.into(),620.into()],
        "AP"=>dictionary! {"N"=>dictionary! {"Gold"=>gold, "Off"=>off}},
        "P"=>page_id
    });
    let leaf1 = document.add_object(dictionary! {
        "Type"=>"Annot","Subtype"=>"Widget","Parent"=>intermediate,
        "AS"=>"Bronze",
        "Rect"=>vec![120.into(),600.into(),148.into(),620.into()],
        "AP"=>dictionary! {"N"=>dictionary! {"Gold"=>gold, "Off"=>off}},
        "P"=>page_id
    });
    if let Some(Object::Dictionary(dict)) = document.objects.get_mut(&intermediate) {
        dict.set("Kids", vec![leaf0.into(), leaf1.into()]);
    }
    if let Some(Object::Dictionary(page)) = document.objects.get_mut(&page_id) {
        page.set("Annots", vec![leaf0.into(), leaf1.into()]);
    }
    let root = document.add_object(dictionary! {
        "FT"=>"Btn","T"=>Object::string_literal("Plan"),"V"=>"Gold","Ff"=>1i64<<15,
        "Kids"=>vec![intermediate.into()]
    });
    let catalog = document.add_object(dictionary! {
        "Type"=>"Catalog","Pages"=>pages_id,
        "AcroForm"=>dictionary! {"Fields"=>vec![root.into()]}
    });
    document.trailer.set("Root", catalog);
    let signals = extract_form_attachment_signals(&document, &[(1, page_id)], true, false);
    let fields = signals.form_fields.expect("fields");
    assert_eq!(
        serde_json::to_value(&fields).unwrap(),
        serde_json::json!([
            {"name":"Plan","id":format_id(root)},
            {"name":"Opt","id":format_id(intermediate)}
        ])
    );
}

#[test]
fn radio_parent_kids_ap_inheritance() {
    let cases = [
        (
            "v3014-form-radio-kids-ap-stream-v1.pdf",
            "Gold",
            serde_json::Value::Null,
        ),
        (
            "v3014-form-radio-kids-apn-stream-v1.pdf",
            "Gold",
            serde_json::Value::Null,
        ),
        (
            "v3014-form-radio-kids-ap-named-v1.pdf",
            "Gold",
            serde_json::json!("Off"),
        ),
    ];
    for (fixture, value, default_value) in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential")
            .join(fixture);
        let document = Document::load(path).expect("load radio parent/kids fixture");
        let pages = document.get_pages().into_iter().collect::<Vec<_>>();
        let output = extract_form_attachment_signals(&document, &pages, true, false);
        let fields = output.form_fields.expect("form fields");
        assert_eq!(fields.len(), 3, "fixture {fixture}");
        assert_eq!(fields[0].name, "Plan", "fixture {fixture}");
        assert!(fields[0].r#type.is_none(), "fixture {fixture}");
        assert!(fields[0].value.is_none(), "fixture {fixture}");
        for kid in &fields[1..] {
            assert_eq!(kid.name, "Plan", "fixture {fixture}");
            assert_eq!(
                kid.r#type.as_deref(),
                Some("radiobutton"),
                "fixture {fixture}"
            );
            assert_eq!(
                serde_json::to_value(&kid.value).unwrap(),
                serde_json::json!(value),
                "fixture {fixture}"
            );
            assert_eq!(
                serde_json::to_value(&kid.default_value).unwrap(),
                default_value,
                "fixture {fixture}"
            );
        }
    }
}
