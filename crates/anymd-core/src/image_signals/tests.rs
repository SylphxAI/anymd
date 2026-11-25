use std::io::Write;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use lopdf::{content::Operation, dictionary, Stream};

use super::decode::{apply_decode, bounded_stream_content, decode_coefficients, to_uint8_clamp};
use super::*;

fn raster_document(
    width: i64,
    height: i64,
    color_space: &str,
    pixels: Vec<u8>,
    paint_count: usize,
) -> (Document, Vec<(u32, ObjectId)>) {
    let mut document = Document::with_version("1.7");
    let image = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width,
            "Height" => height,
            "ColorSpace" => color_space,
            "BitsPerComponent" => 8,
        },
        pixels,
    ));
    let operations = (0..paint_count)
        .map(|_| Operation::new("Do", vec![Object::Name(b"Im0".to_vec())]))
        .collect::<Vec<_>>();
    let content = Content { operations }.encode().expect("encode content");
    let content = document.add_object(Stream::new(dictionary! {}, content));
    let page = document.add_object(dictionary! {
        "Type" => "Page",
        "Resources" => dictionary! {
            "XObject" => dictionary! {"Im0" => image},
        },
        "Contents" => content,
    });
    (document, vec![(1, page)])
}

#[test]
fn extracts_gray_and_rgb_pixels_as_bounded_png_payloads() {
    for (color_space, pixels, format) in [
        ("DeviceGray", vec![0, 85, 170, 255], "rgb"),
        (
            "DeviceRGB",
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
            "rgb",
        ),
    ] {
        let (document, pages) = raster_document(2, 2, color_space, pixels, 1);
        let signals = extract_image_signals(&document, &pages, &[1]);
        assert!(signals.warnings.is_empty());
        assert_eq!(signals.images.len(), 1);
        assert_eq!(signals.images[0]["page"], 1);
        assert_eq!(signals.images[0]["index"], 0);
        assert_eq!(signals.images[0]["width"], 2);
        assert_eq!(signals.images[0]["height"], 2);
        assert_eq!(signals.images[0]["format"], format);
        let encoded = BASE64
            .decode(signals.images[0]["data"].as_str().expect("base64"))
            .expect("decode base64");
        assert_eq!(&encoded[..8], b"\x89PNG\r\n\x1a\n");
    }
}

#[test]
fn repeated_shared_object_preserves_paint_indexes_and_accepts_exact_image_cap() {
    let pixels = vec![17; 1024 * 1024];
    let (document, pages) =
        raster_document(1024, 1024, "DeviceGray", pixels, MAX_IMAGES_PER_SOURCE);
    let signals = extract_image_signals(&document, &pages, &[1]);
    assert!(signals.warnings.is_empty());
    assert_eq!(signals.images.len(), MAX_IMAGES_PER_SOURCE);
    assert_eq!(signals.images.first().unwrap()["index"], 0);
    assert_eq!(
        signals.images.last().unwrap()["index"],
        MAX_IMAGES_PER_SOURCE - 1
    );
    assert!(signals
        .images
        .windows(2)
        .all(|pair| pair[0]["data"] == pair[1]["data"]));
}

#[test]
fn image_cap_plus_one_is_truncated_with_exact_public_warning() {
    let (document, pages) = raster_document(1, 1, "DeviceGray", vec![0], MAX_IMAGES_PER_SOURCE + 1);
    let signals = extract_image_signals(&document, &pages, &[1]);
    assert_eq!(signals.images.len(), MAX_IMAGES_PER_SOURCE);
    assert_eq!(signals.warnings, vec![LIMIT_WARNING]);
}

#[test]
fn selection_isolated_and_unsupported_color_space_fails_closed() {
    let (document, pages) = raster_document(1, 1, "DeviceCMYK", vec![0, 0, 0, 0], 1);
    let unselected = extract_image_signals(&document, &pages, &[]);
    assert!(unselected.images.is_empty());
    assert!(unselected.warnings.is_empty());

    let selected = extract_image_signals(&document, &pages, &[1]);
    assert!(selected.images.is_empty());
    assert_eq!(selected.warnings, vec![UNSUPPORTED_WARNING]);
}

#[test]
fn encoded_output_budget_accepts_exact_cap_and_rejects_cap_plus_one() {
    let mut exact = Budget {
        encoded_bytes: MAX_ENCODED_BYTES_PER_SOURCE - 1,
        ..Budget::default()
    };
    assert!(admit_encoded_output(&mut exact, 1));
    assert_eq!(exact.encoded_bytes, MAX_ENCODED_BYTES_PER_SOURCE);
    assert!(!exact.limited);
    assert!(!admit_encoded_output(&mut exact, 1));
    assert!(exact.limited);
    assert_eq!(exact.encoded_bytes, MAX_ENCODED_BYTES_PER_SOURCE);
}

#[test]
fn decoded_budgets_accept_exact_caps_and_reject_cap_plus_one() {
    let mut per_image = Budget::default();
    assert!(reserve_decoded_image(
        &mut per_image,
        MAX_PIXELS_PER_IMAGE,
        MAX_DECODED_BYTES_PER_IMAGE
    ));
    assert!(!per_image.limited);

    let mut aggregate = Budget {
        pixels: MAX_PIXELS_PER_SOURCE - 1,
        decoded_bytes: MAX_DECODED_BYTES_PER_SOURCE - 1,
        ..Budget::default()
    };
    assert!(reserve_decoded_image(&mut aggregate, 1, 1));
    assert_eq!(aggregate.pixels, MAX_PIXELS_PER_SOURCE);
    assert_eq!(aggregate.decoded_bytes, MAX_DECODED_BYTES_PER_SOURCE);
    assert!(!reserve_decoded_image(&mut aggregate, 1, 1));
    assert!(aggregate.limited);
}

#[test]
fn flate_expansion_is_bounded_before_content_materialization() {
    let expanded = vec![b'A'; MAX_CONTENT_BYTES_PER_PAGE + 1];
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&expanded).expect("compress bomb fixture");
    let compressed = encoder.finish().expect("finish compressed fixture");
    let stream = Stream::new(dictionary! {"Filter" => "FlateDecode"}, compressed);
    assert!(matches!(
        bounded_stream_content(&stream, MAX_CONTENT_BYTES_PER_PAGE),
        Err(DecodeFailure::Limit)
    ));
}

#[test]
fn pixel_limit_plus_one_and_dimension_overflow_fail_before_decode() {
    let (document, pages) = raster_document(
        (MAX_PIXELS_PER_IMAGE + 1) as i64,
        1,
        "DeviceGray",
        Vec::new(),
        1,
    );
    let too_large = extract_image_signals(&document, &pages, &[1]);
    assert!(too_large.images.is_empty());
    assert_eq!(too_large.warnings, vec![LIMIT_WARNING]);

    let (document, pages) = raster_document(i64::MAX, 2, "DeviceRGB", Vec::new(), 1);
    let overflow = extract_image_signals(&document, &pages, &[1]);
    assert!(overflow.images.is_empty());
    assert_eq!(overflow.warnings, vec![LIMIT_WARNING]);
}

#[test]
fn truncated_pixel_buffer_fails_closed_without_internal_error_text() {
    let (document, pages) = raster_document(2, 2, "DeviceRGB", vec![255, 0, 0], 1);
    let signals = extract_image_signals(&document, &pages, &[1]);
    assert!(signals.images.is_empty());
    assert_eq!(signals.warnings, vec![UNSUPPORTED_WARNING]);
}

#[test]
fn resource_admission_is_exact_and_precedes_reference_map_growth() {
    let build = |count: usize| {
        let mut document = Document::with_version("1.7");
        let target = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 1,
                "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            vec![0],
        ));
        let mut xobjects = Dictionary::new();
        for index in 0..count {
            xobjects.set(format!("Im{index}"), target);
        }
        let page = document.add_object(dictionary! {
            "Type" => "Page",
            "Resources" => dictionary! {"XObject" => xobjects},
        });
        (document, page)
    };

    let (document, page) = build(MAX_XOBJECT_RESOURCES_PER_SOURCE);
    let mut exact = Budget::default();
    let resources = page_xobjects(&document, page, &mut exact);
    assert_eq!(resources.len(), MAX_XOBJECT_RESOURCES_PER_SOURCE);
    assert!(!exact.limited);

    let (document, page) = build(MAX_XOBJECT_RESOURCES_PER_SOURCE + 1);
    let mut overflow = Budget::default();
    let resources = page_xobjects(&document, page, &mut overflow);
    assert_eq!(resources.len(), MAX_XOBJECT_RESOURCES_PER_SOURCE);
    assert!(overflow.limited);
}

#[test]
fn nearest_resources_dictionary_shadows_ancestor_xobjects() {
    let mut document = Document::with_version("1.7");
    let ancestor_image = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1,
            "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        },
        vec![0],
    ));
    let direct_image = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1,
            "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        },
        vec![255],
    ));
    let pages = document.add_object(dictionary! {
        "Type" => "Pages",
        "Resources" => dictionary! {"XObject" => dictionary! {"Ancestor" => ancestor_image}},
    });
    let direct_page = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "Resources" => dictionary! {"XObject" => dictionary! {"Direct" => direct_image}},
    });
    let inherited_page = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
    });
    let partially_inherited_page = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "Resources" => dictionary! {"ProcSet" => vec![Object::Name(b"PDF".to_vec())]},
    });

    let mut direct_budget = Budget::default();
    let direct = page_xobjects(&document, direct_page, &mut direct_budget);
    assert_eq!(direct.len(), 1);
    assert_eq!(direct.get(b"Direct".as_slice()), Some(&direct_image));
    assert!(!direct.contains_key(b"Ancestor".as_slice()));

    let mut inherited_budget = Budget::default();
    let inherited = page_xobjects(&document, inherited_page, &mut inherited_budget);
    assert_eq!(inherited.len(), 1);
    assert_eq!(inherited.get(b"Ancestor".as_slice()), Some(&ancestor_image));

    let mut partial_budget = Budget::default();
    let partial = page_xobjects(&document, partially_inherited_page, &mut partial_budget);
    assert_eq!(partial.len(), 1);
    assert_eq!(partial.get(b"Ancestor".as_slice()), Some(&ancestor_image));
}

#[test]
fn applies_device_gray_decode_array_with_javascript_uint8_clamping() {
    let mut document = Document::with_version("1.7");
    let image = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1,
            "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            "Decode" => vec![1.into(), 0.into()],
        },
        vec![0],
    ));
    let content = Content {
        operations: vec![Operation::new("Do", vec![Object::Name(b"Im0".to_vec())])],
    }
    .encode()
    .expect("encode content");
    let content = document.add_object(Stream::new(dictionary! {}, content));
    let page = document.add_object(dictionary! {
        "Type" => "Page",
        "Resources" => dictionary! {"XObject" => dictionary! {"Im0" => image}},
        "Contents" => content,
    });

    let signals = extract_image_signals(&document, &[(1, page)], &[1]);
    assert!(signals.warnings.is_empty());
    let encoded = BASE64
        .decode(signals.images[0]["data"].as_str().expect("base64"))
        .expect("decode base64");
    let rgba = image::load_from_memory(&encoded)
        .expect("decode PNG")
        .to_rgba8();
    assert_eq!(rgba.as_raw(), &[255, 255, 255, 255]);
    assert_eq!(to_uint8_clamp(2.5), 2);
    assert_eq!(to_uint8_clamp(3.5), 4);
}

#[test]
fn rgb_decode_is_per_channel_short_arrays_are_identity_and_bad_values_fail_closed() {
    let document = Document::with_version("1.7");
    let dictionary = dictionary! {
        "Decode" => vec![
            1.into(), 0.into(), 0.into(), 1.into(), 1.into(), 0.into(),
        ],
    };
    let mut budget = Budget::default();
    let coefficients =
        decode_coefficients(&document, &dictionary, 3, &mut budget).expect("RGB decode");
    assert_eq!(
        apply_decode(vec![0, 128, 255], 3, &coefficients),
        [255, 128, 0]
    );
    assert!(!budget.unsupported);

    let short = dictionary! {"Decode" => vec![0.into(), 1.into(), 0.into()]};
    let mut short_budget = Budget::default();
    assert_eq!(
        decode_coefficients(&document, &short, 3, &mut short_budget),
        Some(vec![(1.0, 0.0); 3])
    );
    assert!(!short_budget.unsupported);

    let malformed = dictionary! {
        "Decode" => vec![
            Object::Name(b"bad".to_vec()), 1.into(), 0.into(), 1.into(), 0.into(), 1.into(),
        ],
    };
    let mut malformed_budget = Budget::default();
    assert!(decode_coefficients(&document, &malformed, 3, &mut malformed_budget).is_none());
    assert!(malformed_budget.unsupported);
}

#[test]
fn resource_ancestry_admission_is_exact_and_cycles_fail_closed() {
    let build = |nodes: usize, cycle: bool| {
        let mut document = Document::with_version("1.7");
        let target_id = (nodes as u32 + 1, 0);
        document.objects.insert(
            target_id,
            Object::Stream(Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image", "Width" => 1,
                    "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                },
                vec![0],
            )),
        );
        for index in 1..=nodes {
            let mut node = dictionary! {"Type" => if index == 1 {"Page"} else {"Pages"}};
            if index == nodes {
                if cycle {
                    node.set("Parent", Object::Reference((1, 0)));
                } else {
                    node.set(
                        "Resources",
                        dictionary! {"XObject" => dictionary! {"Im" => target_id}},
                    );
                }
            } else {
                node.set("Parent", Object::Reference((index as u32 + 1, 0)));
            }
            document
                .objects
                .insert((index as u32, 0), Object::Dictionary(node));
        }
        document
    };

    let exact = build(MAX_RESOURCE_ANCESTRY_NODES_PER_SOURCE, false);
    let mut exact_budget = Budget::default();
    let exact_resources = page_xobjects(&exact, (1, 0), &mut exact_budget);
    assert_eq!(exact_resources.len(), 1);
    assert!(!exact_budget.limited);

    let overflow = build(MAX_RESOURCE_ANCESTRY_NODES_PER_SOURCE + 1, false);
    let mut overflow_budget = Budget::default();
    assert!(page_xobjects(&overflow, (1, 0), &mut overflow_budget).is_empty());
    assert!(overflow_budget.limited);

    let cyclic = build(2, true);
    let mut cycle_budget = Budget::default();
    assert!(page_xobjects(&cyclic, (1, 0), &mut cycle_budget).is_empty());
    assert!(cycle_budget.unsupported);
}

#[test]
fn paint_operation_admission_accepts_exact_cap_and_rejects_cap_plus_one() {
    let build = |count: usize| {
        let mut document = Document::with_version("1.7");
        let operations = (0..count)
            .map(|_| Operation::new("Do", vec![Object::Name(b"Missing".to_vec())]))
            .collect::<Vec<_>>();
        let content = Content { operations }.encode().expect("encode content");
        let content = document.add_object(Stream::new(dictionary! {}, content));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Resources" => dictionary! {}, "Contents" => content,
        });
        (document, vec![(1, page)])
    };

    let (document, pages) = build(MAX_IMAGE_OPERATIONS_PER_SOURCE);
    let exact = extract_image_signals(&document, &pages, &[1]);
    assert!(exact.images.is_empty());
    assert!(exact.warnings.is_empty());

    let (document, pages) = build(MAX_IMAGE_OPERATIONS_PER_SOURCE + 1);
    let overflow = extract_image_signals(&document, &pages, &[1]);
    assert!(overflow.images.is_empty());
    assert_eq!(overflow.warnings, vec![LIMIT_WARNING]);
}
