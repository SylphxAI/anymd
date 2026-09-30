//! Embedded raster images: which ones matter, where they sit, what they are
//! captioned, and the bytes to export.
//!
//! An image XObject is exported only when it can carry content: at least
//! 48 x 48 px, at least 2% of the page, and not the same picture on three or
//! more pages (a logo or a running header). JPEG data is exported as it is;
//! 8-bit gray, RGB and palette rasters are re-encoded as PNG. Other encodings
//! (CMYK, JPEG 2000, JBIG2, CCITT, 1-bit masks) are skipped: vector figures
//! and those encodings are not covered yet.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::Read;

use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use pdf_extract::{Dictionary, Document, Object, ObjectId, Stream};

use crate::extract::{Glyph, RawPage};
use crate::rows::{row_text, rows_of, segments_of_row};

/// Images with more pixels than this are refused, before any decoding.
pub const MAX_PIXELS: u64 = 50_000_000;
/// Images narrower or shorter than this are decoration.
pub const MIN_SIDE_PX: u32 = 48;
/// Images covering less of the page than this are decoration.
const MIN_PAGE_FRACTION: f64 = 0.02;
/// An image covering at least this much of a page with no usable text layer is
/// the page itself (a scan), not a figure: the page goes to OCR instead.
const SCAN_PAGE_FRACTION: f64 = 0.8;
/// A page with fewer letters and digits than this has no usable text layer:
/// it is sent to OCR, and an image over it is a scan, not a figure.
pub const SPARSE_PAGE_CHARS: usize = 24;
/// The same picture on this many pages is a logo or header.
const REPEAT_PAGES: usize = 3;
/// The largest encoded image stream read.
const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;
/// A caption sits within this many points of its image.
const CAPTION_REACH: f64 = 36.0;
const CAPTION_LINES: usize = 4;
const CAPTION_CHARS: usize = 200;

/// An image ready to be written to a file.
#[derive(Debug, Clone)]
pub struct EncodedImage {
    pub bytes: Vec<u8>,
    /// File extension without the dot: "jpg" or "png".
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
}

/// An image written by the caller's `place` function.
#[derive(Debug, Clone)]
pub struct Placed {
    /// Absolute path of the exported file.
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// The Markdown that points at it.
    pub markdown: String,
}

/// How embedded images are exported. `place` receives the encoded image, its
/// caption when the page has one, and the page number; it writes the file and
/// returns the Markdown, or `None` to leave the image out.
pub struct ImageOptions<'a> {
    /// Images that repeat across the document (see [`repeated_images`]).
    pub repeated: &'a HashSet<u64>,
    pub place: &'a dyn Fn(&EncodedImage, Option<&str>, u32) -> Option<Placed>,
}

/// An exported image and where it sits on its page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageImage {
    pub page: u32,
    /// Page space, points, y up: x0, y0, x1, y1.
    pub bbox: [f64; 4],
    pub width: u32,
    pub height: u32,
    pub caption: Option<String>,
    pub path: String,
}

/// An exported image, placed in the page's reading order.
#[derive(Debug, Clone)]
pub(crate) struct Figure {
    pub(crate) bbox: [f64; 4],
    pub(crate) markdown: String,
}

fn deref<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    doc.dereference(object).ok().map(|(_, object)| object)
}

fn entry<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    dict.get(key).ok().and_then(|object| deref(doc, object))
}

fn int(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<i64> {
    entry(doc, dict, key)?.as_i64().ok()
}

fn image_stream(doc: &Document, id: ObjectId) -> Option<&Stream> {
    let stream = doc.get_object(id).ok()?.as_stream().ok()?;
    let subtype = stream.dict.get(b"Subtype").ok()?.as_name().ok()?;
    (subtype == &b"Image"[..]).then_some(stream)
}

fn dims(doc: &Document, stream: &Stream) -> Option<(u32, u32)> {
    let width = u32::try_from(int(doc, &stream.dict, b"Width")?).ok()?;
    let height = u32::try_from(int(doc, &stream.dict, b"Height")?).ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

/// Identity of a picture: its size and encoded bytes.
fn content_key(stream: &Stream, width: u32, height: u32) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    stream.content.hash(&mut hasher);
    hasher.finish()
}

fn page_resources(doc: &Document, page_id: ObjectId) -> Option<&Dictionary> {
    let mut current = doc.get_dictionary(page_id).ok()?;
    for _ in 0..32 {
        if let Some(resources) = entry(doc, current, b"Resources").and_then(|o| o.as_dict().ok()) {
            return Some(resources);
        }
        let parent = current.get(b"Parent").ok()?.as_reference().ok()?;
        current = doc.get_dictionary(parent).ok()?;
    }
    None
}

/// Image XObjects a resource dictionary names, looking into form XObjects.
fn collect_images(
    doc: &Document,
    resources: &Dictionary,
    depth: usize,
    seen: &mut HashSet<ObjectId>,
    out: &mut Vec<ObjectId>,
) {
    let Some(xobjects) = entry(doc, resources, b"XObject").and_then(|o| o.as_dict().ok()) else {
        return;
    };
    for (_, value) in xobjects.iter() {
        let Ok(id) = value.as_reference() else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let Some(stream) = doc.get_object(id).ok().and_then(|o| o.as_stream().ok()) else {
            continue;
        };
        match stream
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
        {
            Some(b"Image") => out.push(id),
            Some(b"Form") if depth < 3 => {
                if let Some(inner) =
                    entry(doc, &stream.dict, b"Resources").and_then(|o| o.as_dict().ok())
                {
                    collect_images(doc, inner, depth + 1, seen, out);
                }
            }
            _ => {}
        }
    }
}

/// The pictures that appear on three or more pages of the document: logos,
/// running headers and page ornaments. Pass the set to [`ImageOptions`].
pub fn repeated_images(doc: &Document) -> HashSet<u64> {
    let mut counts: HashMap<u64, usize> = HashMap::new();
    let mut keys: HashMap<ObjectId, Option<u64>> = HashMap::new();
    for (_, page_id) in doc.get_pages() {
        let Some(resources) = page_resources(doc, page_id) else {
            continue;
        };
        let mut objects = Vec::new();
        collect_images(doc, resources, 0, &mut HashSet::new(), &mut objects);
        let mut page_keys = HashSet::new();
        for id in objects {
            let key = *keys.entry(id).or_insert_with(|| {
                let stream = image_stream(doc, id)?;
                let (width, height) = dims(doc, stream)?;
                Some(content_key(stream, width, height))
            });
            if let Some(key) = key {
                page_keys.insert(key);
            }
        }
        for key in page_keys {
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, pages)| *pages >= REPEAT_PAGES)
        .map(|(key, _)| key)
        .collect()
}

/// Export the images that matter on one page and place them.
pub(crate) fn figures_for_page(
    doc: &Document,
    page: &RawPage,
    options: &ImageOptions<'_>,
) -> Vec<(Figure, PageImage)> {
    let glyphs = page.glyphs.as_ref().ok();
    let letters = glyphs.map_or(0, |glyphs| {
        glyphs
            .iter()
            .flat_map(|glyph| glyph.text.chars())
            .filter(|c| c.is_alphanumeric())
            .count()
    });
    let no_text_layer = letters < SPARSE_PAGE_CHARS;
    let mut out: Vec<(Figure, PageImage)> = Vec::new();
    for placement in &page.images {
        let Some(stream) = image_stream(doc, placement.object) else {
            continue;
        };
        let Some((width, height)) = dims(doc, stream) else {
            continue;
        };
        if width < MIN_SIDE_PX
            || height < MIN_SIDE_PX
            || u64::from(width) * u64::from(height) > MAX_PIXELS
        {
            continue;
        }
        let [x0, y0, x1, y1] = placement.bbox;
        if page.area > 0.0 && (x1 - x0) * (y1 - y0) < page.area * MIN_PAGE_FRACTION {
            continue;
        }
        // A scanned page: the image is the page. It emits no ref, so the page
        // stays sparse and goes to OCR exactly as it does without images.
        if no_text_layer
            && page.area > 0.0
            && (x1 - x0) * (y1 - y0) >= page.area * SCAN_PAGE_FRACTION
        {
            continue;
        }
        if options
            .repeated
            .contains(&content_key(stream, width, height))
        {
            continue;
        }
        // The same picture painted twice in one place is one figure.
        if out.iter().any(|(figure, _)| {
            figure
                .bbox
                .iter()
                .zip(&placement.bbox)
                .all(|(a, b)| (a - b).abs() < 1.0)
        }) {
            continue;
        }
        let Some(image) = encode(doc, stream, width, height) else {
            continue;
        };
        let caption = glyphs.and_then(|glyphs| caption_near(glyphs, placement.bbox));
        let Some(placed) = (options.place)(&image, caption.as_deref(), page.number) else {
            continue;
        };
        out.push((
            Figure {
                bbox: placement.bbox,
                markdown: placed.markdown,
            },
            PageImage {
                page: page.number,
                bbox: placement.bbox,
                width: placed.width,
                height: placed.height,
                caption,
                path: placed.path,
            },
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Captions

/// "Figure 3", "Fig. 3", "Table 2", "圖1", "图 1", "表3" at the start of a line.
pub(crate) fn is_caption_start(text: &str) -> bool {
    let lower = text.trim_start().to_lowercase();
    let rest = ["figure", "fig.", "fig", "table", "圖", "图", "表"]
        .iter()
        .find_map(|prefix| lower.strip_prefix(*prefix));
    let Some(rest) = rest else {
        return false;
    };
    rest.trim_start()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || ('０'..='９').contains(&c))
}

struct Line {
    x0: f64,
    x1: f64,
    top: f64,
    bottom: f64,
    size: f64,
    text: String,
}

/// The caption of the image at `bbox`: the nearest line directly below or
/// above it that starts like a caption, with the lines that continue it.
fn caption_near(glyphs: &[Glyph], bbox: [f64; 4]) -> Option<String> {
    let mut lines: Vec<Line> = Vec::new();
    for row in rows_of(glyphs.to_vec()) {
        let segments = segments_of_row(row);
        let text = row_text(&segments);
        if segments.is_empty() || text.trim().is_empty() {
            continue;
        }
        lines.push(Line {
            x0: segments.iter().map(|s| s.x0).fold(f64::INFINITY, f64::min),
            x1: segments
                .iter()
                .map(|s| s.x1)
                .fold(f64::NEG_INFINITY, f64::max),
            top: segments
                .iter()
                .map(|s| s.top)
                .fold(f64::NEG_INFINITY, f64::max),
            bottom: segments
                .iter()
                .map(|s| s.bottom)
                .fold(f64::INFINITY, f64::min),
            size: segments.iter().map(|s| s.size).fold(0.0, f64::max),
            text,
        });
    }
    let [ix0, iy0, ix1, iy1] = bbox;
    let reach = -2.0..=CAPTION_REACH;
    let mut best: Option<(f64, String)> = None;
    for (index, line) in lines.iter().enumerate() {
        if !is_caption_start(&line.text) || !(line.x1 > ix0 && line.x0 < ix1) {
            continue;
        }
        // Lines directly beneath with the same type size continue the caption.
        let mut block = vec![index];
        let mut bottom = line.bottom;
        while block.len() < CAPTION_LINES {
            let next = lines
                .iter()
                .enumerate()
                .filter(|(i, l)| {
                    !block.contains(i)
                        && l.top <= bottom + 1.0
                        && bottom - l.top <= 0.7 * line.size
                        && (l.size - line.size).abs() <= 1.0
                        && l.x1 > line.x0 - line.size
                        && l.x0 < line.x1 + line.size
                        && !is_caption_start(&l.text)
                })
                .max_by(|a, b| a.1.top.total_cmp(&b.1.top));
            let Some((i, l)) = next else {
                break;
            };
            block.push(i);
            bottom = l.bottom;
        }
        let below = iy0 - line.top;
        let above = bottom - iy1;
        let gap = if reach.contains(&below) {
            below
        } else if reach.contains(&above) {
            above
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|(best_gap, _)| gap < *best_gap) {
            let text: Vec<&str> = block.iter().map(|i| lines[*i].text.as_str()).collect();
            best = Some((gap, text.join(" ")));
        }
    }
    let (_, text) = best?;
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > CAPTION_CHARS {
        let cut: String = text.chars().take(CAPTION_CHARS).collect();
        return Some(format!("{}…", cut.trim_end()));
    }
    Some(text)
}

// ---------------------------------------------------------------------------
// Encoding

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

fn decode_params<'a>(doc: &'a Document, dict: &'a Dictionary) -> Option<&'a Dictionary> {
    let params = entry(doc, dict, b"DecodeParms").or_else(|| entry(doc, dict, b"DP"))?;
    match params {
        Object::Dictionary(dict) => Some(dict),
        Object::Array(items) => deref(doc, items.first()?)?.as_dict().ok(),
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
fn encode(doc: &Document, stream: &Stream, width: u32, height: u32) -> Option<EncodedImage> {
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
