//! Glyphs and ruling lines from the PDF content stream (via `pdf-extract`).

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::{MAX_GLYPHS_PER_PAGE, MAX_WORKERS};
use pdf_extract::{output_doc_page, ColorSpace, Document, MediaBox, OutputDev, OutputError, Path as PdfPath, Transform};

#[derive(Debug, Clone)]
pub(crate) struct Glyph {
    /// Start and end along the text direction.
    pub(crate) x0: f64,
    pub(crate) x1: f64,
    /// Baseline position across the text direction (larger = higher on page).
    pub(crate) base: f64,
    pub(crate) size: f64,
    pub(crate) text: String,
    pub(crate) space: bool,
}

pub(crate) struct RawPage {
    pub(crate) number: u32,
    pub(crate) bottom: f64,
    pub(crate) top: f64,
    pub(crate) glyphs: Result<Vec<Glyph>, String>,
    pub(crate) rotated: Vec<Glyph>,
}

#[derive(Default)]
pub(crate) struct Collector {
    pub(crate) media: Option<MediaBox>,
    pub(crate) glyphs: Vec<Glyph>,
    pub(crate) rotated: Vec<Glyph>,
}

impl OutputDev for Collector {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.media = Some(*media_box);
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        character: &str,
    ) -> Result<(), OutputError> {
        if self.glyphs.len() + self.rotated.len() >= MAX_GLYPHS_PER_PAGE {
            return Err(OutputError::IoError(std::io::Error::other(
                "page has too many glyphs",
            )));
        }
        let values = [
            trm.m11, trm.m12, trm.m21, trm.m22, trm.m31, trm.m32, width, font_size,
        ];
        if !values.iter().all(|value| value.is_finite()) {
            return Ok(());
        }
        let text = normalize_glyph_text(character);
        if text.is_empty() {
            return Ok(());
        }
        let size = (trm.m21.hypot(trm.m22) * font_size).abs();
        if size <= 0.1 || size > 2000.0 {
            return Ok(());
        }
        let scale = trm.m11.hypot(trm.m12);
        if scale <= 0.0 {
            return Ok(());
        }
        let (dx, dy) = (trm.m11 / scale, trm.m12 / scale);
        let space = text.chars().all(char::is_whitespace);
        // Character spacing (Tc) is part of the advance; word spacing is not.
        let tracking = if space { 0.0 } else { spacing };
        let mut advance = ((width * font_size + tracking) * scale).abs();
        if advance < size * 0.05 {
            // Fonts without widths would otherwise glue every glyph together.
            advance = size * 0.5;
        }
        let upright = dx > 0.9 && dy.abs() < 0.2;
        let (along, across) = if upright {
            (trm.m31, trm.m32)
        } else {
            // Project onto the rotated text direction.
            (trm.m31 * dx + trm.m32 * dy, -trm.m31 * dy + trm.m32 * dx)
        };
        let glyph = Glyph {
            x0: along,
            x1: along + advance,
            base: across,
            size,
            text: if space { " ".into() } else { text },
            space,
        };
        if upright {
            self.glyphs.push(glyph);
        } else {
            self.rotated.push(glyph);
        }
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn stroke(
        &mut self,
        _ctm: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        _path: &PdfPath,
    ) -> Result<(), OutputError> {
        Ok(())
    }

    fn fill(
        &mut self,
        _ctm: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        _path: &PdfPath,
    ) -> Result<(), OutputError> {
        Ok(())
    }
}

pub(crate) fn normalize_glyph_text(character: &str) -> String {
    let mut out = String::with_capacity(character.len());
    for ch in character.chars() {
        match ch {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{3000}' | '\t' => out.push(' '),
            '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{FEFF}' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn extract_pages(doc: &Document, selected: &[u32]) -> Vec<RawPage> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, MAX_WORKERS)
        .min(selected.len().max(1));
    let extract_one = |number: u32| -> RawPage {
        let mut collector = Collector::default();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            output_doc_page(doc, &mut collector, number)
        }));
        let glyphs = match outcome {
            Ok(Ok(())) => Ok(std::mem::take(&mut collector.glyphs)),
            Ok(Err(err)) => Err(format!("page {number}: text extraction failed ({err})")),
            Err(_) => Err(format!(
                "page {number}: text extraction failed (malformed font or content)"
            )),
        };
        let (bottom, top) = collector
            .media
            .map(|media| (media.lly.min(media.ury), media.lly.max(media.ury)))
            .unwrap_or((0.0, 792.0));
        RawPage {
            number,
            bottom,
            top,
            glyphs,
            rotated: collector.rotated,
        }
    };
    if workers <= 1 {
        return selected.iter().map(|&number| extract_one(number)).collect();
    }
    let mut results: Vec<Option<RawPage>> = (0..selected.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let extract_one = &extract_one;
                scope.spawn(move || {
                    selected
                        .iter()
                        .enumerate()
                        .skip(worker)
                        .step_by(workers)
                        .map(|(index, &number)| (index, extract_one(number)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            if let Ok(pages) = handle.join() {
                for (index, page) in pages {
                    results[index] = Some(page);
                }
            }
        }
    });
    results
        .into_iter()
        .zip(selected)
        .map(|(page, &number)| {
            page.unwrap_or(RawPage {
                number,
                bottom: 0.0,
                top: 792.0,
                glyphs: Err(format!("page {number}: text extraction failed")),
                rotated: Vec::new(),
            })
        })
        .collect()
}
