use lopdf::{dictionary, Document, Object};
use serde_json::json;

use super::{document_with_pages, extract_page_signals};

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
