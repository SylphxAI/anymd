use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Read};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use flate2::read::ZlibDecoder;
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use lopdf::{content::Content, Dictionary, Document, Object, ObjectId, Stream};
use serde_json::{json, Value};

const MAX_IMAGES_PER_SOURCE: usize = 128;
const MAX_IMAGE_OPERATIONS_PER_SOURCE: usize = 4_096;
const MAX_XOBJECT_RESOURCES_PER_SOURCE: usize = 16_384;
const MAX_RESOURCE_ANCESTRY_NODES_PER_SOURCE: usize = 4_096;
const MAX_RESOURCE_NAME_BYTES: usize = 1_024;
const MAX_CONTENT_BYTES_PER_PAGE: usize = 4 * 1024 * 1024;
const MAX_PIXELS_PER_IMAGE: usize = 16 * 1024 * 1024;
const MAX_PIXELS_PER_SOURCE: usize = 32 * 1024 * 1024;
const MAX_DECODED_BYTES_PER_IMAGE: usize = 64 * 1024 * 1024;
const MAX_DECODED_BYTES_PER_SOURCE: usize = 128 * 1024 * 1024;
const MAX_ENCODED_BYTES_PER_SOURCE: usize = 64 * 1024 * 1024;

const LIMIT_WARNING: &str =
    "include_images: common raster extraction reached a bounded resource limit; remaining images were omitted.";
const UNSUPPORTED_WARNING: &str =
    "include_images: unsupported or malformed image XObjects were omitted from the bounded common-raster subset.";

#[derive(Default)]
pub(crate) struct ImageSignals {
    pub images: Vec<Value>,
    pub warnings: Vec<String>,
}

#[derive(Clone)]
struct DecodedImage {
    width: usize,
    height: usize,
    format: &'static str,
    data: String,
    encoded_bytes: usize,
}

#[derive(Default)]
struct Budget {
    operations: usize,
    pixels: usize,
    decoded_bytes: usize,
    encoded_bytes: usize,
    resource_entries: usize,
    resource_ancestry_nodes: usize,
    limited: bool,
    unsupported: bool,
}

pub(crate) fn extract_image_signals(
    document: &Document,
    pages: &[(u32, ObjectId)],
    selected_pages: &[u32],
) -> ImageSignals {
    let selected = selected_pages.iter().copied().collect::<HashSet<_>>();
    let mut output = ImageSignals::default();
    let mut cache = HashMap::<ObjectId, Option<DecodedImage>>::new();
    let mut budget = Budget::default();

    'pages: for (page_number, page_id) in pages {
        if !selected.contains(page_number) {
            continue;
        }
        let resources = page_xobjects(document, *page_id, &mut budget);
        if budget.limited {
            break;
        }
        let content = match bounded_page_content(document, *page_id) {
            Ok(content) => content,
            Err(DecodeFailure::Limit) => {
                budget.limited = true;
                continue;
            }
            Err(DecodeFailure::Unsupported) => {
                budget.unsupported = true;
                continue;
            }
        };
        let Ok(operations) = Content::decode(&content).map(|content| content.operations) else {
            budget.unsupported = true;
            continue;
        };
        let mut page_index = 0usize;
        for operation in operations {
            if operation.operator != "Do" {
                continue;
            }
            budget.operations = budget.operations.saturating_add(1);
            if budget.operations > MAX_IMAGE_OPERATIONS_PER_SOURCE
                || output.images.len() >= MAX_IMAGES_PER_SOURCE
            {
                budget.limited = true;
                break 'pages;
            }
            let Some(name) = operation
                .operands
                .first()
                .and_then(|value| value.as_name().ok())
            else {
                budget.unsupported = true;
                continue;
            };
            let operation_index = page_index;
            page_index += 1;
            let Some(object_id) = resources.get(name) else {
                // Form XObjects and inline/direct objects are deliberately outside this slice.
                continue;
            };
            let decoded = if let Some(cached) = cache.get(object_id) {
                cached.clone()
            } else {
                let value = decode_image(document, *object_id, &mut budget);
                cache.insert(*object_id, value.clone());
                value
            };
            let Some(decoded) = decoded else {
                continue;
            };
            if !admit_encoded_output(&mut budget, decoded.encoded_bytes) {
                break 'pages;
            }
            // Decoded pixels are charged once per unique object; encoded payload bytes are
            // charged per emitted paint operation because each public image part duplicates it.
            output.images.push(json!({
                "page": page_number,
                "index": operation_index,
                "width": decoded.width,
                "height": decoded.height,
                "format": decoded.format,
                "data": decoded.data,
            }));
        }
    }

    if budget.limited {
        output.warnings.push(LIMIT_WARNING.into());
    }
    if budget.unsupported {
        output.warnings.push(UNSUPPORTED_WARNING.into());
    }
    output
}

fn page_xobjects(
    document: &Document,
    page_id: ObjectId,
    budget: &mut Budget,
) -> BTreeMap<Vec<u8>, ObjectId> {
    let mut output = BTreeMap::new();
    let mut current = page_id;
    let mut seen = HashSet::new();
    loop {
        budget.resource_ancestry_nodes = budget.resource_ancestry_nodes.saturating_add(1);
        if budget.resource_ancestry_nodes > MAX_RESOURCE_ANCESTRY_NODES_PER_SOURCE {
            budget.limited = true;
            return output;
        }
        if !seen.insert(current) {
            budget.unsupported = true;
            return output;
        }
        let Ok(node) = document.get_dictionary(current) else {
            budget.unsupported = true;
            return output;
        };
        if let Ok(resources) = node.get(b"Resources") {
            if let Some(resources) = resolve_dictionary(document, resources) {
                // PDF.js merges inheritable Resources at the top level, without
                // merging sub-dictionaries. Continue until the nearest dictionary
                // that owns XObject; that key then shadows all ancestor XObjects.
                if resources.get(b"XObject").is_ok() {
                    collect_xobjects(document, resources, &mut output, budget);
                    return output;
                }
            } else {
                budget.unsupported = true;
                return output;
            }
        }
        let Some(parent) = node
            .get(b"Parent")
            .ok()
            .and_then(|value| value.as_reference().ok())
        else {
            return output;
        };
        current = parent;
    }
}

fn resolve_dictionary<'a>(document: &'a Document, value: &'a Object) -> Option<&'a Dictionary> {
    resolve(document, value).ok()?.as_dict().ok()
}

fn collect_xobjects(
    document: &Document,
    resources: &Dictionary,
    output: &mut BTreeMap<Vec<u8>, ObjectId>,
    budget: &mut Budget,
) {
    let Some(dictionary) = resources
        .get(b"XObject")
        .ok()
        .and_then(|value| resolve(document, value).ok())
        .and_then(|value| value.as_dict().ok())
    else {
        return;
    };
    for (name, value) in dictionary {
        budget.resource_entries = budget.resource_entries.saturating_add(1);
        if budget.resource_entries > MAX_XOBJECT_RESOURCES_PER_SOURCE {
            budget.limited = true;
            return;
        }
        if name.len() > MAX_RESOURCE_NAME_BYTES {
            budget.unsupported = true;
            continue;
        }
        let Object::Reference(object_id) = value else {
            continue;
        };
        output.entry(name.clone()).or_insert(*object_id);
    }
}

#[derive(Clone, Copy)]
enum DecodeFailure {
    Limit,
    Unsupported,
}

fn bounded_page_content(document: &Document, page_id: ObjectId) -> Result<Vec<u8>, DecodeFailure> {
    let mut output = Vec::new();
    for object_id in document.get_page_contents(page_id) {
        let stream = document
            .get_object(object_id)
            .ok()
            .and_then(|value| value.as_stream().ok())
            .ok_or(DecodeFailure::Unsupported)?;
        let remaining = MAX_CONTENT_BYTES_PER_PAGE.saturating_sub(output.len());
        let bytes = bounded_stream_content(stream, remaining)?;
        output.extend_from_slice(&bytes);
    }
    Ok(output)
}

fn bounded_stream_content(stream: &Stream, limit: usize) -> Result<Vec<u8>, DecodeFailure> {
    let filters = if stream.dict.get(b"Filter").is_err() {
        Vec::new()
    } else {
        stream.filters().map_err(|_| DecodeFailure::Unsupported)?
    };
    if stream.dict.get(b"DecodeParms").is_ok() {
        return Err(DecodeFailure::Unsupported);
    }
    match filters.as_slice() {
        [] => {
            if stream.content.len() > limit {
                Err(DecodeFailure::Limit)
            } else {
                Ok(stream.content.clone())
            }
        }
        [b"FlateDecode"] => {
            let mut decoder = ZlibDecoder::new(Cursor::new(&stream.content));
            let mut output = Vec::new();
            decoder
                .by_ref()
                .take(limit.saturating_add(1) as u64)
                .read_to_end(&mut output)
                .map_err(|_| DecodeFailure::Unsupported)?;
            if output.len() > limit {
                Err(DecodeFailure::Limit)
            } else {
                Ok(output)
            }
        }
        _ => Err(DecodeFailure::Unsupported),
    }
}

fn decode_image(
    document: &Document,
    object_id: ObjectId,
    budget: &mut Budget,
) -> Option<DecodedImage> {
    let stream = document.get_object(object_id).ok()?.as_stream().ok()?;
    if stream
        .dict
        .get(b"Subtype")
        .ok()
        .and_then(|value| value.as_name().ok())
        != Some(b"Image")
    {
        return None;
    }
    let width = positive_usize(stream.dict.get(b"Width").ok()?)?;
    let height = positive_usize(stream.dict.get(b"Height").ok()?)?;
    if stream
        .dict
        .get(b"BitsPerComponent")
        .ok()
        .and_then(|value| value.as_i64().ok())
        != Some(8)
    {
        budget.unsupported = true;
        return None;
    }
    let (input_channels, format, color) = match stream
        .dict
        .get(b"ColorSpace")
        .ok()
        .and_then(|value| resolve(document, value).ok())
        .and_then(|value| value.as_name().ok())
    {
        // PDF.js expands this common DeviceGray XObject path to RGB before
        // processImageData, so the public v3.0.14 format is `rgb`.
        Some(b"DeviceGray") | Some(b"G") => (1usize, "rgb", ExtendedColorType::Rgb8),
        Some(b"DeviceRGB") | Some(b"RGB") => (3usize, "rgb", ExtendedColorType::Rgb8),
        _ => {
            budget.unsupported = true;
            return None;
        }
    };
    let Some(pixels) = width.checked_mul(height) else {
        budget.limited = true;
        return None;
    };
    let Some(input_bytes) = pixels.checked_mul(input_channels) else {
        budget.limited = true;
        return None;
    };
    let Some(materialized_bytes) = pixels.checked_mul(3) else {
        budget.limited = true;
        return None;
    };
    if !reserve_decoded_image(budget, pixels, materialized_bytes) {
        return None;
    }
    let input = match bounded_stream_content(stream, input_bytes) {
        Ok(bytes) if bytes.len() == input_bytes => bytes,
        Ok(_) | Err(DecodeFailure::Unsupported) => {
            budget.unsupported = true;
            return None;
        }
        Err(DecodeFailure::Limit) => {
            budget.limited = true;
            return None;
        }
    };
    let decode = decode_coefficients(document, &stream.dict, input_channels, budget)?;
    let input = apply_decode(input, input_channels, &decode);
    let pixels_data = if input_channels == 1 {
        let mut rgb = Vec::with_capacity(materialized_bytes);
        for gray in input {
            rgb.extend_from_slice(&[gray, gray, gray]);
        }
        rgb
    } else {
        input
    };
    let mut png = Vec::new();
    if PngEncoder::new(&mut png)
        .write_image(&pixels_data, width as u32, height as u32, color)
        .is_err()
    {
        budget.unsupported = true;
        return None;
    }
    let data = BASE64.encode(&png);
    let encoded_bytes = data.len();
    Some(DecodedImage {
        width,
        height,
        format,
        data,
        encoded_bytes,
    })
}

fn decode_coefficients(
    document: &Document,
    dictionary: &Dictionary,
    channels: usize,
    budget: &mut Budget,
) -> Option<Vec<(f64, f64)>> {
    let Ok(value) = dictionary.get(b"Decode").or_else(|_| dictionary.get(b"D")) else {
        return Some(vec![(1.0, 0.0); channels]);
    };
    let Some(values) = resolve(document, value)
        .ok()
        .and_then(|value| value.as_array().ok())
    else {
        budget.unsupported = true;
        return None;
    };
    if values.len() < channels.saturating_mul(2) {
        return Some(vec![(1.0, 0.0); channels]);
    }
    let mut coefficients = Vec::with_capacity(channels);
    for pair in values.chunks_exact(2).take(channels) {
        let Some(minimum) = pdf_number(document, &pair[0]) else {
            budget.unsupported = true;
            return None;
        };
        let Some(maximum) = pdf_number(document, &pair[1]) else {
            budget.unsupported = true;
            return None;
        };
        coefficients.push((maximum - minimum, 255.0 * minimum));
    }
    Some(coefficients)
}

fn pdf_number(document: &Document, value: &Object) -> Option<f64> {
    match resolve(document, value).ok()? {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn apply_decode(mut input: Vec<u8>, channels: usize, coefficients: &[(f64, f64)]) -> Vec<u8> {
    for (index, sample) in input.iter_mut().enumerate() {
        let (coefficient, addend) = coefficients[index % channels];
        *sample = to_uint8_clamp(addend + f64::from(*sample) * coefficient);
    }
    input
}

fn to_uint8_clamp(value: f64) -> u8 {
    if value <= 0.0 {
        return 0;
    }
    if value >= 255.0 {
        return 255;
    }
    let floor = value.floor();
    let fraction = value - floor;
    if fraction < 0.5 || (fraction == 0.5 && (floor as u8).is_multiple_of(2)) {
        floor as u8
    } else {
        floor as u8 + 1
    }
}

fn reserve_decoded_image(budget: &mut Budget, pixels: usize, decoded_bytes: usize) -> bool {
    let Some(total_pixels) = budget.pixels.checked_add(pixels) else {
        budget.limited = true;
        return false;
    };
    let Some(total_bytes) = budget.decoded_bytes.checked_add(decoded_bytes) else {
        budget.limited = true;
        return false;
    };
    if pixels > MAX_PIXELS_PER_IMAGE
        || decoded_bytes > MAX_DECODED_BYTES_PER_IMAGE
        || total_pixels > MAX_PIXELS_PER_SOURCE
        || total_bytes > MAX_DECODED_BYTES_PER_SOURCE
    {
        budget.limited = true;
        return false;
    }
    budget.pixels = total_pixels;
    budget.decoded_bytes = total_bytes;
    true
}

fn admit_encoded_output(budget: &mut Budget, bytes: usize) -> bool {
    let Some(total) = budget.encoded_bytes.checked_add(bytes) else {
        budget.limited = true;
        return false;
    };
    if total > MAX_ENCODED_BYTES_PER_SOURCE {
        budget.limited = true;
        return false;
    }
    budget.encoded_bytes = total;
    true
}

fn positive_usize(value: &Object) -> Option<usize> {
    let value = value.as_i64().ok()?;
    usize::try_from(value).ok().filter(|value| *value > 0)
}

fn resolve<'a>(document: &'a Document, value: &'a Object) -> lopdf::Result<&'a Object> {
    match value {
        Object::Reference(id) => document.get_object(*id),
        value => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{content::Operation, dictionary};
    use std::io::Write;

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
        let (document, pages) =
            raster_document(1, 1, "DeviceGray", vec![0], MAX_IMAGES_PER_SOURCE + 1);
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
}
