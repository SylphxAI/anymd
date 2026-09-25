use super::*;
use lopdf::dictionary;
use serde_json::json;

fn tagged_document() -> (Document, Vec<(u32, ObjectId)>, ObjectId) {
    let mut document = Document::with_version("1.7");
    let pages_root = document.new_object_id();
    let page_one = document.new_object_id();
    let page_two = document.new_object_id();
    let annotation = document.new_object_id();
    let struct_root = document.new_object_id();
    let heading = document.new_object_id();
    let figure = document.new_object_id();
    let parent_tree = document.new_object_id();
    document.set_object(
        pages_root,
        dictionary! {"Type"=>"Pages","Kids"=>vec![page_one.into(),page_two.into()],"Count"=>2},
    );
    document.set_object(
        page_one,
        dictionary! {"Type"=>"Page","Parent"=>pages_root,"StructParents"=>0,"Annots"=>vec![annotation.into()]},
    );
    document.set_object(page_two, dictionary! {"Type"=>"Page","Parent"=>pages_root});
    document.set_object(
        annotation,
        dictionary! {"Type"=>"Annot","Subtype"=>"Link","StructParent"=>1},
    );
    document.set_object(
        heading,
        dictionary! {"Type"=>"StructElem","S"=>"CustomHeading","P"=>struct_root,"Pg"=>page_one,"K"=>0},
    );
    document.set_object(
        figure,
        dictionary! {"Type"=>"StructElem","S"=>"Figure","P"=>struct_root,"Pg"=>page_one,
        "K"=>dictionary!{"Type"=>"OBJR","Pg"=>page_one,"Obj"=>annotation}},
    );
    document.set_object(
        parent_tree,
        dictionary! {"Nums"=>vec![0.into(),Object::Array(vec![heading.into(),figure.into()]),1.into(),figure.into()]},
    );
    document.set_object(
        struct_root,
        dictionary! {"Type"=>"StructTreeRoot","K"=>vec![heading.into(),figure.into()],
        "ParentTree"=>parent_tree,"RoleMap"=>dictionary!{"CustomHeading"=>"H1"}},
    );
    let catalog = document.add_object(
        dictionary! {"Type"=>"Catalog","Pages"=>pages_root,"StructTreeRoot"=>struct_root},
    );
    document.trailer.set("Root", catalog);
    (document, vec![(1, page_one), (2, page_two)], annotation)
}

#[test]
fn extracts_role_content_annotation_and_empty_root_pages_in_selected_order() {
    let (document, pages, annotation) = tagged_document();
    let trees = extract_structure_trees(&document, &pages, &[2, 1, 1]);
    assert_eq!(
        serde_json::to_value(trees).unwrap(),
        json!([
            {"page":1,"tree":{"role":"Root","children":[
                {"role":"H1","children":[{"type":"content","id":format!("p{}_mc0",format_id(pages[0].1))}]},
                {"role":"Figure","children":[{"type":"annotation","id":format!("pdfjs_internal_id_{}",format_id(annotation))}]}
            ]}},
            {"page":2,"tree":{"role":"Root"}}
        ])
    );
}

#[test]
fn scalar_k_references_preserve_structural_identity_and_content_resolution() {
    let (mut document, pages, _) = tagged_document();
    let catalog_id = document
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let struct_root = document.objects[&catalog_id]
        .as_dict()
        .unwrap()
        .get(b"StructTreeRoot")
        .unwrap()
        .as_reference()
        .unwrap();
    let root_kids = document.objects[&struct_root]
        .as_dict()
        .unwrap()
        .get(b"K")
        .unwrap()
        .as_array()
        .unwrap();
    let heading = root_kids[0].as_reference().unwrap();
    let generated_heading = (heading.0, 7);
    let heading_object = document.objects.remove(&heading).unwrap();
    document.objects.insert(generated_heading, heading_object);
    let parent_tree = document.objects[&struct_root]
        .as_dict()
        .unwrap()
        .get(b"ParentTree")
        .unwrap()
        .as_reference()
        .unwrap();
    let mcid = document.add_object(Object::Integer(0));
    document
        .objects
        .get_mut(&generated_heading)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("K", mcid);
    document
        .objects
        .get_mut(&struct_root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("K", generated_heading);
    document
        .objects
        .get_mut(&parent_tree)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Nums",
            vec![
                Object::Integer(0),
                Object::Array(vec![Object::Reference(generated_heading)]),
            ],
        );

    assert_eq!(
        serde_json::to_value(extract_structure_trees(&document, &pages, &[1, 2])).unwrap(),
        json!([
            {"page":1,"tree":{"role":"Root","children":[
                {"role":"H1","children":[{"type":"content","id":format!("p{}_mc0",format_id(pages[0].1))}]}
            ]}},
            {"page":2,"tree":{"role":"Root"}}
        ])
    );

    document
        .objects
        .get_mut(&parent_tree)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Nums",
            vec![Object::Integer(0), Object::Reference(generated_heading)],
        );
    assert_eq!(
        serde_json::to_value(extract_structure_trees(&document, &pages, &[1, 2])).unwrap(),
        json!([
            {"page":1,"tree":{"role":"Root"}},
            {"page":2,"tree":{"role":"Root"}}
        ])
    );
}

#[test]
fn direct_top_level_k_keeps_empty_roots_without_admitting_indirect_parent_tree_nodes() {
    let (mut document, pages, _) = tagged_document();
    let catalog_id = document
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let struct_root = document.objects[&catalog_id]
        .as_dict()
        .unwrap()
        .get(b"StructTreeRoot")
        .unwrap()
        .as_reference()
        .unwrap();
    document
        .objects
        .get_mut(&struct_root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "K",
            Object::Dictionary(dictionary! {
                "Type"=>"StructElem","S"=>"H1","P"=>struct_root,"Pg"=>pages[0].1,"K"=>0
            }),
        );

    assert_eq!(
        serde_json::to_value(extract_structure_trees(&document, &pages, &[1, 2])).unwrap(),
        json!([
            {"page":1,"tree":{"role":"Root"}},
            {"page":2,"tree":{"role":"Root"}}
        ])
    );
}

#[test]
fn untagged_and_page_local_failures_do_not_invent_trees() {
    let mut document = Document::with_version("1.7");
    let pages =
        document.add_object(dictionary! {"Type"=>"Pages","Kids"=>Vec::<Object>::new(),"Count"=>0});
    let catalog = document.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    document.trailer.set("Root", catalog);
    assert!(extract_structure_trees(&document, &[], &[1]).is_empty());

    let (mut document, pages, _) = tagged_document();
    let page = document
        .objects
        .get_mut(&pages[0].1)
        .unwrap()
        .as_dict_mut()
        .unwrap();
    page.set("StructParents", 999);
    page.remove(b"Annots");
    let trees = extract_structure_trees(&document, &pages, &[1, 2]);
    assert_eq!(trees.len(), 2);
    assert!(serde_json::to_value(&trees[0]).unwrap()["tree"]
        .get("children")
        .is_none());
}

#[test]
fn parent_tree_orphan_absent_from_root_k_is_rejected_but_empty_pages_remain_valid() {
    let (mut document, pages, _) = tagged_document();
    let catalog_id = document
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let struct_root = document.objects[&catalog_id]
        .as_dict()
        .unwrap()
        .get(b"StructTreeRoot")
        .unwrap()
        .as_reference()
        .unwrap();
    let parent_tree = document.objects[&struct_root]
        .as_dict()
        .unwrap()
        .get(b"ParentTree")
        .unwrap()
        .as_reference()
        .unwrap();
    let orphan = document.add_object(
        dictionary! {"Type"=>"StructElem","S"=>"H1","P"=>struct_root,"Pg"=>pages[0].1,"K"=>0},
    );
    document
        .objects
        .get_mut(&parent_tree)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Nums",
            vec![Object::Integer(0), Object::Array(vec![orphan.into()])],
        );

    let extraction = extract_structure_trees_checked(&document, &pages, &[1, 2])
        .unwrap()
        .unwrap();
    assert!(!extraction.complete);
    assert_eq!(
        serde_json::to_value(extraction.trees).unwrap(),
        json!([{"page":2,"tree":{"role":"Root"}}])
    );
}

#[test]
fn public_normalizer_matches_frozen_golden() {
    let oracle: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/structure-normalizer-golden.json"
    ))
    .unwrap();
    assert_eq!(
        normalize_public_tree(&oracle["input"]),
        Some(oracle["expected"].clone())
    );
    assert!(normalize_public_tree(&json!(null)).is_none());
}

#[test]
fn hostile_page_is_omitted_without_suppressing_other_selected_pages() {
    let (mut document, pages, _) = tagged_document();
    let oversized = (0..=MAX_ARRAY_ITEMS)
        .map(|_| Object::Null)
        .collect::<Vec<_>>();
    document
        .objects
        .get_mut(&pages[0].1)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Annots", Object::Array(oversized));

    let extraction = extract_structure_trees_checked(&document, &pages, &[1, 2])
        .unwrap()
        .unwrap();
    assert!(!extraction.complete);
    assert_eq!(
        serde_json::to_value(extraction.trees).unwrap(),
        json!([
            {"page":2,"tree":{"role":"Root"}}
        ])
    );
}

#[test]
fn public_normalizer_fails_closed_beyond_pdfjs_depth() {
    let mut raw = json!({"role":"P"});
    for _ in 0..=PDFJS_MAX_DEPTH {
        raw = json!({"role":"P","children":[raw]});
    }
    let normalized = normalize_public_tree(&raw).unwrap();
    let mut cursor = &normalized;
    for _ in 0..=PDFJS_MAX_DEPTH {
        let Some(child) = cursor
            .get("children")
            .and_then(Value::as_array)
            .and_then(|children| children.first())
        else {
            return;
        };
        cursor = child;
    }
    panic!("normalizer admitted a node beyond the pdf.js depth boundary");
}

#[test]
fn array_admission_is_exact_and_precedes_materialization() {
    let document = Document::with_version("1.7");
    let exact = Object::Array(vec![Object::Null; MAX_REQUEST_WORK]);
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert_eq!(
        object_list(&mut walker, &exact).unwrap().len(),
        MAX_REQUEST_WORK
    );
    assert_eq!(budget.admitted, MAX_REQUEST_WORK);
    assert_eq!(budget.materialized, MAX_REQUEST_WORK);

    let oversized = Object::Array(vec![Object::Null; MAX_REQUEST_WORK + 1]);
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert!(object_list(&mut walker, &oversized).is_err());
    assert_eq!(budget.admitted, 0);
    assert_eq!(budget.materialized, 0);
}

#[test]
fn node_work_and_selected_page_admission_have_exact_request_boundaries() {
    let document = Document::with_version("1.7");
    let mut budget = RequestBudget::default();
    {
        let mut walker = Walker::new(&document, &mut budget);
        for _ in 0..MAX_REQUEST_WORK {
            assert!(walker.admit_node().is_some());
        }
        assert!(walker.admit_node().is_none());
    }
    assert_eq!(budget.admitted, MAX_REQUEST_WORK);
    assert_eq!(budget.materialized, 0);

    let mut budget = RequestBudget::default();
    for _ in 0..MAX_REQUEST_WORK {
        assert!(budget.admit_work().is_some());
    }
    assert!(budget.admit_work().is_none());
    assert_eq!(budget.work, MAX_REQUEST_WORK);

    let mut budget = RequestBudget::default();
    assert!(budget.admit_items(MAX_REQUEST_WORK).is_some());
    assert!(budget.admit_items(1).is_none());
    assert_eq!(budget.admitted, MAX_REQUEST_WORK);
    assert_eq!(budget.materialized, 0);

    let mut budget = RequestBudget::default();
    assert!(budget.admit_text(MAX_ROLE_MAP_TEXT_BYTES).is_some());
    assert!(budget.admit_text(1).is_none());
    assert_eq!(budget.text_bytes, MAX_ROLE_MAP_TEXT_BYTES);
}

#[test]
fn repeated_shared_huge_roles_are_cumulatively_bounded_before_output_cloning() {
    let (mut document, pages, _) = tagged_document();
    let catalog_id = document
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let struct_root = document.objects[&catalog_id]
        .as_dict()
        .unwrap()
        .get(b"StructTreeRoot")
        .unwrap()
        .as_reference()
        .unwrap();
    let top_level = document.objects[&struct_root]
        .as_dict()
        .unwrap()
        .get(b"K")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_reference().unwrap())
        .collect::<Vec<_>>();
    let huge_role = document.add_object(Object::Name(vec![b'R'; 600_000]));
    for id in top_level {
        document
            .objects
            .get_mut(&id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("S", huge_role);
    }

    let extraction = extract_structure_trees_checked(&document, &pages, &[1, 2])
        .unwrap()
        .unwrap();
    assert!(!extraction.complete);
    assert_eq!(
        serde_json::to_value(extraction.trees).unwrap(),
        json!([{"page":2,"tree":{"role":"Root"}}])
    );
    assert_eq!(lossy_utf8_len(&[0xff, b'a', 0xfe]), 7);
}

#[test]
fn parent_tree_nums_exact_array_boundary_counts_duplicates_and_invalid_values() {
    let document = Document::with_version("1.7");
    let mut nums = Vec::with_capacity(MAX_ARRAY_ITEMS);
    for index in 0..MAX_ARRAY_ITEMS / 2 {
        nums.push(Object::Integer((index % 2) as i64));
        nums.push(if index % 3 == 0 {
            Object::Null
        } else {
            Object::Name(b"invalid-direct".to_vec())
        });
    }
    let root = Object::Dictionary(dictionary! {"Nums"=>nums});
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    let tree = read_number_tree(&mut walker, &root).unwrap();
    assert_eq!(tree.len(), 2);
    assert_eq!(budget.admitted, MAX_ARRAY_ITEMS);
    assert_eq!(budget.materialized, MAX_ARRAY_ITEMS + 1);

    let oversized =
        Object::Dictionary(dictionary! {"Nums"=>vec![Object::Null; MAX_ARRAY_ITEMS + 1]});
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert!(read_number_tree(&mut walker, &oversized).is_none());
    assert_eq!(budget.admitted, 0);
    assert_eq!(budget.materialized, 1);
}

#[test]
fn indirect_arrays_and_reference_depth_are_bounded_before_copying() {
    let mut document = Document::with_version("1.7");
    let array = document.add_object(Object::Array(vec![Object::Null; MAX_REQUEST_WORK]));
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert_eq!(
        object_list(&mut walker, &Object::Reference(array))
            .unwrap()
            .len(),
        MAX_REQUEST_WORK
    );
    assert_eq!(budget.work, 1);
    assert_eq!(budget.materialized, MAX_REQUEST_WORK);

    let mut document = Document::with_version("1.7");
    let terminal = document.add_object(Object::Integer(7));
    let mut current = terminal;
    for _ in 0..PDFJS_MAX_DEPTH - 1 {
        current = document.add_object(Object::Reference(current));
    }
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert_eq!(
        walker.resolve(&Object::Reference(current)),
        Some(&Object::Integer(7))
    );

    current = document.add_object(Object::Reference(current));
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    let too_deep = Object::Reference(current);
    assert!(walker.resolve(&too_deep).is_none());
    assert!(walker.limited);
}

#[test]
fn role_map_admission_and_text_budget_precede_cloning() {
    let document = Document::with_version("1.7");
    let mut exact = Dictionary::new();
    for index in 0..MAX_REQUEST_WORK {
        exact.set(format!("R{index}"), Object::Name(b"P".to_vec()));
    }
    let root = dictionary! {"RoleMap"=>exact};
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert_eq!(read_role_map(&mut walker, &root).len(), MAX_REQUEST_WORK);
    assert_eq!(budget.materialized, MAX_REQUEST_WORK);

    let mut oversized = Dictionary::new();
    for index in 0..=MAX_REQUEST_WORK {
        oversized.set(format!("R{index}"), Object::Name(b"P".to_vec()));
    }
    let root = dictionary! {"RoleMap"=>oversized};
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert!(read_role_map(&mut walker, &root).is_empty());
    assert_eq!(budget.materialized, 0);

    let huge = vec![b'x'; MAX_ROLE_MAP_TEXT_BYTES + 1];
    let root = dictionary! {"RoleMap"=>dictionary!{b"R".to_vec()=>Object::Name(huge)}};
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert!(read_role_map(&mut walker, &root).is_empty());
    assert_eq!(budget.materialized, 0);
}

#[test]
fn parent_tree_fanout_and_shared_cycles_fail_before_queue_growth() {
    let mut document = Document::with_version("1.7");
    let leaf = document.add_object(dictionary! {"Nums"=>Vec::<Object>::new()});
    let shared =
        document.add_object(dictionary! {"Kids"=>vec![Object::Reference(leaf); MAX_ARRAY_ITEMS]});
    let root = dictionary! {"Kids"=>vec![Object::Reference(shared); MAX_REQUEST_WORK]};
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    assert!(read_number_tree(&mut walker, &Object::Dictionary(root)).is_none());
    assert_eq!(budget.admitted, MAX_REQUEST_WORK);
    assert_eq!(budget.materialized, MAX_REQUEST_WORK + 1);

    let root_id = document.new_object_id();
    document.set_object(
        root_id,
        dictionary! {"Kids"=>vec![Object::Reference(root_id)]},
    );
    let mut budget = RequestBudget::default();
    let mut walker = Walker::new(&document, &mut budget);
    let root_reference = Object::Reference(root_id);
    assert!(read_number_tree(&mut walker, &root_reference).is_none());
    assert!(walker.failed);
}

#[test]
fn cumulative_page_budget_does_not_reset_between_selected_pages() {
    let (mut document, pages, _) = tagged_document();
    for (_, page_id) in &pages {
        document
            .objects
            .get_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", Object::Array(vec![Object::Null; 6_000]));
    }
    let trees = extract_structure_trees(&document, &pages, &[1, 2]);
    assert_eq!(trees.len(), 1);
    assert_eq!(serde_json::to_value(&trees[0]).unwrap()["page"], 1);
}

#[test]
fn cyclic_ancestry_is_not_exposed_as_an_empty_valid_root() {
    let (mut document, pages, _) = tagged_document();
    let struct_root = document
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .ok()
        .and_then(|catalog| document.objects.get(&catalog))
        .and_then(|value| value.as_dict().ok())
        .and_then(|catalog| catalog.get(b"StructTreeRoot").ok())
        .and_then(|value| value.as_reference().ok())
        .unwrap();
    let kids = document.objects[&struct_root]
        .as_dict()
        .unwrap()
        .get(b"K")
        .unwrap()
        .as_array()
        .unwrap();
    let heading = kids[0].as_reference().unwrap();
    let figure = kids[1].as_reference().unwrap();
    document
        .objects
        .get_mut(&heading)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("P", figure);
    document
        .objects
        .get_mut(&heading)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("K", figure);
    document
        .objects
        .get_mut(&figure)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("P", heading);
    document
        .objects
        .get_mut(&figure)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("K", heading);
    let trees = extract_structure_trees(&document, &pages, &[1, 2]);
    assert_eq!(
        serde_json::to_value(trees).unwrap(),
        json!([
            {"page":2,"tree":{"role":"Root"}}
        ])
    );
}

#[test]
fn serializes_mcr_and_non_annotation_objr_ids() {
    let (mut document, pages, _) = tagged_document();
    let struct_root = document
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let catalog = document.objects[&struct_root].as_dict().unwrap();
    let tree_root = catalog
        .get(b"StructTreeRoot")
        .unwrap()
        .as_reference()
        .unwrap();
    let heading = document.objects[&tree_root]
        .as_dict()
        .unwrap()
        .get(b"K")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .as_reference()
        .unwrap();
    let object = document.add_object(dictionary! {"Type"=>"XObject"});
    document
        .objects
        .get_mut(&heading)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "K",
            vec![
                Object::Integer(0),
                Object::Dictionary(dictionary! {"Type"=>"MCR","Pg"=>pages[0].1,"MCID"=>1}),
                Object::Dictionary(dictionary! {"Type"=>"OBJR","Pg"=>pages[0].1,"Obj"=>object}),
            ],
        );
    let value = serde_json::to_value(extract_structure_trees(&document, &pages, &[1])).unwrap();
    assert_eq!(
        value[0]["tree"]["children"][0]["children"],
        json!([
            {"type":"content","id":format!("p{}_mc0",format_id(pages[0].1))},
            {"type":"content","id":format!("p{}_mc1",format_id(pages[0].1))},
            {"type":"object","id":format_id(object)}
        ])
    );
}
