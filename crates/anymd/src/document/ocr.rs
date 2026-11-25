use std::path::Path;

use anymd_core::markdown_layout::{self, SPARSE_PAGE_CHARS};

use super::{Opened, Unit};

/// Render scale for OCR: 300 dpi, the resolution tesseract is trained for.
const OCR_SCALE: f32 = 300.0 / 72.0;
const OCR_MAX_PIXELS: u64 = 40_000_000;

impl Opened {
    pub(crate) fn note_ocr_ran(&self, page: u32) {
        if let Ok(mut ran) = self.ocr_ran.lock() {
            ran.push(page);
        }
    }

    /// Whether OCR ran on this page (so a blank result means it found no text).
    pub fn ocr_ran(&self, page: u32) -> bool {
        self.ocr_ran.lock().is_ok_and(|ran| ran.contains(&page))
    }

    pub(super) fn ocr_sparse_pages(
        &self,
        units: &mut [Unit],
        doc: &markdown_layout::PdfDocument,
        bytes: Option<&Vec<u8>>,
        path: Option<&Path>,
    ) -> Result<(), String> {
        let wanted = match self.options.ocr {
            Some(false) => return Ok(()),
            Some(true) => true,
            None => false,
        };
        let vlm = crate::ocr_vlm::requested(self.options.ocr_engine.unwrap_or_default());
        let sparse = ocr_candidates(units, doc, vlm);
        if sparse.is_empty() {
            return Ok(());
        }
        let mut admission = if vlm {
            Some(
                self.ocr_request
                    .lock()
                    .map_err(|_| "OCR request state poisoned".to_string())?,
            )
        } else {
            None
        };
        if let Some(admission) = admission.as_mut() {
            if admission.is_none() {
                **admission = Some(crate::ocr_vlm::OcrRequest::acquire()?);
            }
        }
        if !vlm && !anymd_formats::image::ocr_available() {
            if wanted {
                for index in sparse {
                    units[index].markdown.push_str(
                        "\n\n<!-- OCR needs `tesseract` installed (apt install tesseract-ocr / brew install tesseract). -->",
                    );
                }
            }
            return Ok(());
        }
        let pdf_bytes = match (bytes, path) {
            (Some(bytes), _) => bytes.clone(),
            (None, Some(path)) => match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) if vlm => return Err(error.to_string()),
                Err(_) => return Ok(()),
            },
            (None, None) => return Ok(()),
        };
        let renderer = match anymd_core::render::RenderDocument::new(pdf_bytes) {
            Ok(renderer) => renderer,
            Err(error) if vlm => return Err(error.message),
            Err(_) => return Ok(()),
        };
        let mut results: Vec<(usize, Result<String, String>)> = Vec::with_capacity(sparse.len());
        if vlm {
            // The document keeps one shared permit/deadline across lazy unit
            // reads. Propagate the first failure before rendering another page.
            let request = admission
                .as_mut()
                .and_then(|guard| guard.as_mut())
                .ok_or("Missing OCR admission")?;
            for &index in &sparse {
                let text = request
                    .run_page(|deadline, permit| {
                        let page = renderer
                            .render_page(
                                units[index].number as usize,
                                OCR_SCALE,
                                OCR_MAX_PIXELS,
                                256 * 1024 * 1024,
                            )
                            .map_err(|error| error.message)?;
                        crate::ocr_vlm::recognize_admitted(&page.png, deadline, permit)
                            .map(|p| p.markdown())
                    })
                    .map_err(|error| {
                        format!(
                            "Doc-VLM OCR failed on page {}: {error}",
                            units[index].number
                        )
                    })?;
                results.push((index, Ok(text)));
            }
        } else {
            // Render one page at a time, then OCR the pages side by side (one
            // tesseract thread each).
            let rendered: Vec<(usize, Result<anymd_core::render::RenderedPage, String>)> = sparse
                .iter()
                .map(|&index| {
                    let page = units[index].number as usize;
                    let image = renderer
                        .render_page(page, OCR_SCALE, OCR_MAX_PIXELS, 256 * 1024 * 1024)
                        .map_err(|error| error.message);
                    (index, image)
                })
                .collect();
            let workers = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .clamp(1, 8);

            for chunk in rendered.chunks(workers) {
                std::thread::scope(|scope| {
                    let handles: Vec<_> = chunk
                        .iter()
                        .map(|(index, image)| {
                            scope.spawn(move || {
                                let text = image.clone().and_then(|image| ocr_page(&image));
                                (*index, text)
                            })
                        })
                        .collect();
                    for handle in handles {
                        if let Ok(result) = handle.join() {
                            results.push(result);
                        }
                    }
                });
            }
        }
        for (index, text) in results {
            match text {
                Ok(text) if !text.trim().is_empty() => {
                    self.note_ocr_ran(units[index].number);
                    let existing = units[index].markdown.trim().to_string();
                    units[index].markdown = if existing.is_empty() {
                        format!("<!-- OCR text -->\n\n{text}")
                    } else {
                        format!("{existing}\n\n<!-- OCR text -->\n\n{text}")
                    };
                }
                Ok(_) => {
                    self.note_ocr_ran(units[index].number);
                }
                Err(error) if vlm => {
                    return Err(format!(
                        "Doc-VLM OCR failed on page {}: {error}",
                        units[index].number
                    ));
                }
                Err(error) => {
                    units[index]
                        .markdown
                        .push_str(&format!("\n\n<!-- OCR failed: {error} -->"));
                }
            }
        }
        Ok(())
    }
}

/// Native raster/text evidence, not emitted Markdown, selects VLM pages.
pub(super) fn ocr_candidates(
    units: &[Unit],
    doc: &markdown_layout::PdfDocument,
    vlm: bool,
) -> Vec<usize> {
    if vlm {
        let numbers: Vec<_> = units.iter().map(|unit| unit.number).collect();
        let image_only = markdown_layout::image_only_pages(doc, &numbers);
        units
            .iter()
            .enumerate()
            .filter(|(_, unit)| image_only.contains(&unit.number))
            .map(|(index, _)| index)
            .collect()
    } else {
        units
            .iter()
            .enumerate()
            .filter(|(_, unit)| visible_chars(&unit.markdown) < SPARSE_PAGE_CHARS)
            .map(|(index, _)| index)
            .collect()
    }
}

/// OCR one rendered page and lay its words out like a text page.
fn ocr_page(image: &anymd_core::render::RenderedPage) -> Result<String, String> {
    let words = anymd_formats::image::ocr_words(&image.png, ".png")?;
    let placed: Vec<markdown_layout::PlacedWord> = words
        .into_iter()
        .map(|word| markdown_layout::PlacedWord {
            x0: f64::from(word.left),
            top: f64::from(word.top),
            x1: f64::from(word.left + word.width),
            bottom: f64::from(word.top + word.height),
            line: (u64::from(word.line.0) << 40)
                | (u64::from(word.line.1) << 20)
                | u64::from(word.line.2),
            text: word.text,
        })
        .collect();
    let points_per_pixel = 1.0 / f64::from(image.scale.max(0.01));
    Ok(markdown_layout::words_to_markdown(
        &placed,
        f64::from(image.height),
        points_per_pixel,
    ))
}

pub(super) fn visible_chars(markdown: &str) -> usize {
    markdown
        .lines()
        .filter(|line| !line.trim_start().starts_with("<!--"))
        .flat_map(str::chars)
        .filter(|c| c.is_alphanumeric())
        .count()
}
