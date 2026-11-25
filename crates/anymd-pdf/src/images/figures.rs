//! Image selection, scan detection, and placement in reading order.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use pdf_extract::{Dictionary, Document, ObjectId, Stream};

use crate::extract::RawPage;

use super::caption::caption_near;
use super::encode::encode;
use super::objects::entry;
use super::{
    dims, image_stream, Figure, ImageOptions, PageImage, MAX_PIXELS, MIN_SIDE_PX, SPARSE_PAGE_CHARS,
};

/// Images covering less of the page than this are decoration.
const MIN_PAGE_FRACTION: f64 = 0.02;
/// An image covering at least this much of a page with no usable text layer is
/// the page itself (a scan), not a figure: the page goes to OCR instead.
const SCAN_PAGE_FRACTION: f64 = 0.8;
/// The same picture on this many pages is a logo or header.
const REPEAT_PAGES: usize = 3;

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

/// The lowest resolution, in pixels per inch over the placed area, at which
/// an image can carry a scanned page.
const MIN_SCAN_DPI: f64 = 50.0;

/// Whether a placed image looks like a scan of a page: both sides at least
/// `MIN_SIDE_PX` and at least `MIN_SCAN_DPI` over the placed area. Reads the
/// image dictionary only and never decodes. A tiny image stretched over the
/// page is not one.
pub(crate) fn is_scan_like(doc: &Document, placement: &crate::extract::Placement) -> bool {
    let Some(stream) = image_stream(doc, placement.object) else {
        return false;
    };
    let Some((width, height)) = dims(doc, stream) else {
        return false;
    };
    if width < MIN_SIDE_PX || height < MIN_SIDE_PX {
        return false;
    }
    let b = placement.bbox;
    let (w, h) = ((b[2] - b[0]).abs(), (b[3] - b[1]).abs());
    w > 0.0
        && h > 0.0
        && f64::from(width) * 72.0 / w >= MIN_SCAN_DPI
        && f64::from(height) * 72.0 / h >= MIN_SCAN_DPI
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
    let no_text_layer = letters < SPARSE_PAGE_CHARS || page.invisible_layer;
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
