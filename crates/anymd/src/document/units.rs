use std::sync::Arc;

use anymd_core::document::scanned_pdf_pages;
use anymd_core::markdown_layout;

use super::cache::{cache_get, cache_key, cache_put, referenced_images, CachedDoc};
use super::images::place_image;
use super::{Body, Opened, Unit};

impl Opened {
    /// Convert the requested units (1-based numbers, ascending).
    pub fn units(&self, numbers: &[u32]) -> Result<Vec<Unit>, String> {
        match &self.body {
            Body::Units(units) => Ok(numbers
                .iter()
                .filter_map(|n| units.get(*n as usize - 1).cloned())
                .collect()),
            Body::Pdf {
                doc,
                bytes,
                path,
                title,
                repeated,
            } => {
                let converted = match &self.options.images {
                    Some(store) => {
                        let repeated =
                            repeated.get_or_init(|| markdown_layout::repeated_images(doc));
                        let place = |image: &markdown_layout::EncodedImage,
                                     caption: Option<&str>,
                                     page: u32| {
                            place_image(store, image, caption, page)
                        };
                        let options = markdown_layout::ImageOptions {
                            repeated,
                            place: &place,
                        };
                        markdown_layout::pdf_to_markdown_with_images(
                            doc,
                            Some(numbers),
                            Some(&options),
                        )
                    }
                    None => markdown_layout::pdf_to_markdown(doc, Some(numbers)),
                }
                .map_err(|error| error.message)?;
                let _ = title;
                let mut units: Vec<Unit> = converted.pages.into_iter().map(Unit::from).collect();
                self.ocr_sparse_pages(&mut units, doc, bytes.as_deref(), path.as_deref())?;
                Ok(units)
            }
        }
    }

    /// PDF pages among `units` with no text (after any OCR) that are covered by
    /// images: scans that were not read, as opposed to genuinely empty pages.
    pub fn scanned_pages(&self, units: &[Unit]) -> Vec<u32> {
        let Body::Pdf { doc, .. } = &self.body else {
            return Vec::new();
        };
        scanned_pdf_pages(doc, units)
    }

    /// Every unit (PDFs convert in parallel chunks). Cached for local files.
    pub fn all_units(&self) -> Result<Arc<Vec<Unit>>, String> {
        match &self.body {
            Body::Units(units) => Ok(units.clone()),
            Body::Pdf { path, .. } => {
                let ocr_on =
                    self.options.ocr != Some(false) && anymd_formats::image::ocr_available();
                let key = path
                    .as_deref()
                    .and_then(|p| cache_key(p, ocr_on, &self.options));
                if let Some(cached) = key.as_ref().and_then(cache_get) {
                    return Ok(cached.units.clone());
                }
                let numbers: Vec<u32> = (1..=self.total).collect();
                let units = Arc::new(self.units(&numbers)?);
                if let Some(key) = key {
                    cache_put(
                        key,
                        Arc::new(CachedDoc {
                            format: "pdf",
                            native_outline: Vec::new(),
                            title: self.title.clone(),
                            metadata: Vec::new(),
                            units: units.clone(),
                            bytes: units.iter().map(|u| u.markdown.len()).sum(),
                            images: referenced_images(&units, self.options.images.as_ref()),
                        }),
                    );
                }
                Ok(units)
            }
        }
    }
}
