//! Export supported image streams as JPEG or PNG.

use std::io::Read;

use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use pdf_extract::{Document, Object, Stream};

use super::objects::{decode_params, deref, int};
use super::EncodedImage;

/// The largest encoded image stream read.
const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;

enum Model {
    Gray,
    Rgb,
    Cmyk,
    Indexed(Vec<[u8; 3]>),
}

impl Model {
    fn components(&self) -> Option<usize> {
        match self {
            Self::Gray | Self::Indexed(_) => Some(1),
            Self::Rgb => Some(3),
            Self::Cmyk => None,
        }
    }
}

fn named_model(name: &[u8]) -> Option<Model> {
    match name {
        b"DeviceGray" | b"CalGray" | b"G" => Some(Model::Gray),
        b"DeviceRGB" | b"CalRGB" | b"RGB" => Some(Model::Rgb),
        b"DeviceCMYK" | b"CMYK" => Some(Model::Cmyk),
        _ => None,
    }
}

fn color_model(doc: &Document, object: &Object, depth: usize) -> Option<Model> {
    if depth > 3 {
        return None;
    }
    match deref(doc, object)? {
        Object::Name(name) => named_model(name),
        Object::Array(items) => {
            let head = deref(doc, items.first()?)?.as_name().ok()?;
            match head {
                b"ICCBased" => {
                    let stream = deref(doc, items.get(1)?)?.as_stream().ok()?;
                    match int(doc, &stream.dict, b"N")? {
                        1 => Some(Model::Gray),
                        3 => Some(Model::Rgb),
                        4 => Some(Model::Cmyk),
                        _ => None,
                    }
                }
                b"Indexed" | b"I" => {
                    let components = match color_model(doc, items.get(1)?, depth + 1)? {
                        Model::Gray => 1,
                        Model::Rgb => 3,
                        _ => return None,
                    };
                    let hival = deref(doc, items.get(2)?)?.as_i64().ok()?;
                    let count = usize::try_from(hival).ok()?.checked_add(1)?;
                    if count > 256 {
                        return None;
                    }
                    let lookup: Vec<u8> = match deref(doc, items.get(3)?)? {
                        Object::String(bytes, _) => bytes.clone(),
                        Object::Stream(stream) if stream.content.len() <= 1 << 20 => {
                            stream.decompressed_content().ok()?
                        }
                        _ => return None,
                    };
                    let palette = (0..count)
                        .map(|i| {
                            if components == 1 {
                                let v = *lookup.get(i)?;
                                Some([v, v, v])
                            } else {
                                Some([
                                    *lookup.get(i * 3)?,
                                    *lookup.get(i * 3 + 1)?,
                                    *lookup.get(i * 3 + 2)?,
                                ])
                            }
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(Model::Indexed(palette))
                }
                other => named_model(other),
            }
        }
        _ => None,
    }
}

fn inflate(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .ok()?;
    (out.len() <= limit).then_some(out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i32::from(a), i32::from(b), i32::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Undo PNG row filters (PDF predictors 10 to 15).
fn png_unfilter(data: &[u8], row: usize, bpp: usize, height: usize) -> Option<Vec<u8>> {
    let stride = row + 1;
    if data.len() < stride.checked_mul(height)? {
        return None;
    }
    let mut out = vec![0u8; row * height];
    for y in 0..height {
        let tag = data[y * stride];
        let src = &data[y * stride + 1..(y + 1) * stride];
        for x in 0..row {
            let a = if x >= bpp { out[y * row + x - bpp] } else { 0 };
            let b = if y > 0 { out[(y - 1) * row + x] } else { 0 };
            let c = if x >= bpp && y > 0 {
                out[(y - 1) * row + x - bpp]
            } else {
                0
            };
            let value = match tag {
                0 => src[x],
                1 => src[x].wrapping_add(a),
                2 => src[x].wrapping_add(b),
                3 => src[x].wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => src[x].wrapping_add(paeth(a, b, c)),
                _ => return None,
            };
            out[y * row + x] = value;
        }
    }
    Some(out)
}

/// The bytes of an image XObject as a file, when it is a kind we export.
pub(super) fn encode(
    doc: &Document,
    stream: &Stream,
    width: u32,
    height: u32,
) -> Option<EncodedImage> {
    if stream.content.len() > MAX_ENCODED_BYTES {
        return None;
    }
    let filters: Vec<&[u8]> = if stream.dict.get(b"Filter").is_ok() {
        stream.filters().ok()?
    } else {
        Vec::new()
    };
    let names: Vec<&str> = filters
        .iter()
        .map(|filter| std::str::from_utf8(filter).unwrap_or(""))
        .collect();
    let model = color_model(doc, stream.dict.get(b"ColorSpace").ok()?, 0)?;
    match names.as_slice() {
        ["DCTDecode" | "DCT"] => {
            if !matches!(model, Model::Gray | Model::Rgb)
                || !stream.content.starts_with(&[0xFF, 0xD8, 0xFF])
            {
                return None;
            }
            Some(EncodedImage {
                bytes: stream.content.clone(),
                ext: "jpg",
                width,
                height,
            })
        }
        [] => raster(doc, stream, false, &model, width, height),
        ["FlateDecode" | "Fl"] => raster(doc, stream, true, &model, width, height),
        _ => None,
    }
}

/// An 8-bit gray, RGB or palette raster as a PNG.
fn raster(
    doc: &Document,
    stream: &Stream,
    compressed: bool,
    model: &Model,
    width: u32,
    height: u32,
) -> Option<EncodedImage> {
    if int(doc, &stream.dict, b"BitsPerComponent")? != 8 {
        return None;
    }
    let components = model.components()?;
    let row = (width as usize).checked_mul(components)?;
    let expected = row.checked_mul(height as usize)?;
    let params = if compressed {
        decode_params(doc, &stream.dict)
    } else {
        None
    };
    let predictor = params.and_then(|p| int(doc, p, b"Predictor")).unwrap_or(1);
    if let Some(columns) = params.and_then(|p| int(doc, p, b"Columns")) {
        if columns != i64::from(width) {
            return None;
        }
    }
    let data = if compressed {
        inflate(&stream.content, expected.checked_add(height as usize)? + 64)?
    } else {
        stream.content.clone()
    };
    let data = match predictor {
        1 => data,
        10..=15 => png_unfilter(&data, row, components, height as usize)?,
        _ => return None,
    };
    if data.len() < expected {
        return None;
    }
    let raw = &data[..expected];
    let expanded: Vec<u8>;
    let (color, pixels): (ExtendedColorType, &[u8]) = match model {
        Model::Gray => (ExtendedColorType::L8, raw),
        Model::Rgb => (ExtendedColorType::Rgb8, raw),
        Model::Indexed(palette) => {
            let last = palette.len().checked_sub(1)?;
            expanded = raw
                .iter()
                .flat_map(|index| palette[usize::from(*index).min(last)])
                .collect();
            (ExtendedColorType::Rgb8, &expanded)
        }
        Model::Cmyk => return None,
    };
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(pixels, width, height, color)
        .ok()?;
    Some(EncodedImage {
        bytes: png,
        ext: "png",
        width,
        height,
    })
}
