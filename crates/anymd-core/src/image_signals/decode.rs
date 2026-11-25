use std::io::{Cursor, Read};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use flate2::read::ZlibDecoder;
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use super::{positive_usize, reserve_decoded_image, resolve, Budget, MAX_CONTENT_BYTES_PER_PAGE};

#[derive(Clone)]
pub(super) struct DecodedImage {
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) format: &'static str,
    pub(super) data: String,
    pub(super) encoded_bytes: usize,
}

#[derive(Clone, Copy)]
pub(super) enum DecodeFailure {
    Limit,
    Unsupported,
}

pub(super) fn bounded_page_content(
    document: &Document,
    page_id: ObjectId,
) -> Result<Vec<u8>, DecodeFailure> {
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

pub(super) fn bounded_stream_content(
    stream: &Stream,
    limit: usize,
) -> Result<Vec<u8>, DecodeFailure> {
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

pub(super) fn decode_image(
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

pub(super) fn decode_coefficients(
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
    for pair in values.as_chunks::<2>().0.iter().take(channels) {
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

pub(super) fn apply_decode(
    mut input: Vec<u8>,
    channels: usize,
    coefficients: &[(f64, f64)],
) -> Vec<u8> {
    for (index, sample) in input.iter_mut().enumerate() {
        let (coefficient, addend) = coefficients[index % channels];
        *sample = to_uint8_clamp(addend + f64::from(*sample) * coefficient);
    }
    input
}

pub(super) fn to_uint8_clamp(value: f64) -> u8 {
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
