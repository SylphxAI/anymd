use pdf_extract::Document;

use crate::extract::extract_pages;

/// Pages containing painted raster images but no native letters or digits.
///
/// Uses the same glyph and image extraction as native Markdown. A short title
/// or a failed text extraction is not evidence of an image-only page.
pub fn image_only_pages(doc: &Document, pages: &[u32]) -> Vec<u32> {
    extract_pages(doc, pages)
        .into_iter()
        .filter(image_only_page)
        .map(|page| page.number)
        .collect()
}

/// Pages that are image-only AND mostly covered by images: scans, as opposed to
/// a blank page carrying a logo or a spacer image. Image boxes (summed, capped
/// at the page area) must cover at least half the page, so a scan split into
/// strips still counts.
pub fn scanned_pages(doc: &Document, pages: &[u32]) -> Vec<u32> {
    extract_pages(doc, pages)
        .into_iter()
        .filter(scanned_page)
        .map(|page| page.number)
        .collect()
}

pub(super) fn scanned_page(page: &crate::extract::RawPage) -> bool {
    if !image_only_page(page) || page.area.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return false;
    }
    let covered: f64 = page
        .images
        .iter()
        .map(|image| {
            (image.bbox[2] - image.bbox[0]).max(0.0) * (image.bbox[3] - image.bbox[1]).max(0.0)
        })
        .sum();
    covered.min(page.area) >= 0.5 * page.area
}

pub(super) fn image_only_page(page: &crate::extract::RawPage) -> bool {
    !page.images.is_empty()
        && page.glyphs.as_ref().is_ok_and(|glyphs| {
            !glyphs
                .iter()
                .chain(&page.rotated)
                .flat_map(|glyph| glyph.text.chars())
                .any(char::is_alphanumeric)
        })
}
