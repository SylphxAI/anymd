//! Glyphs and ruling lines from the PDF content stream (via `pdf-extract`).

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::MAX_WORKERS;
use pdf_extract::{output_doc_page, Document, ObjectId};

mod collector;
mod rules;
mod visibility;

pub(crate) use collector::Collector;

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

/// A straight horizontal or vertical line drawn on the page: a stroked
/// segment, a thin filled bar, or an edge of a filled box. `at` is the y of a
/// horizontal rule or the x of a vertical one; `from..to` is its extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rule {
    pub(crate) horizontal: bool,
    pub(crate) at: f64,
    pub(crate) from: f64,
    pub(crate) to: f64,
    /// An edge of a shaded box rather than a drawn line. A shaded column or
    /// band does not divide cells the way a drawn line does.
    pub(crate) soft: bool,
}

/// An image XObject painted on a page: its object and page-space extent
/// (x0, y0, x1, y1; y up).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placement {
    pub(crate) object: ObjectId,
    pub(crate) bbox: [f64; 4],
}

/// Images painted on one page beyond this many are ignored.
const MAX_PLACEMENTS_PER_PAGE: usize = 256;

pub(crate) struct RawPage {
    pub(crate) number: u32,
    pub(crate) bottom: f64,
    pub(crate) top: f64,
    pub(crate) glyphs: Result<Vec<Glyph>, String>,
    pub(crate) rotated: Vec<Glyph>,
    pub(crate) rules: Vec<Rule>,
    /// Glyphs made from OCR word boxes: even widths inside a word say nothing
    /// about the font being monospace.
    pub(crate) ocr: bool,
    /// Image XObjects painted on the page.
    pub(crate) images: Vec<Placement>,
    /// The page's area in square points (0 when unknown).
    pub(crate) area: f64,
    /// Embedded images kept for the output, placed in reading order.
    pub(crate) figures: Vec<crate::images::Figure>,
    /// The page's text includes an invisible OCR text layer over a scan.
    pub(crate) invisible_layer: bool,
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

/// Returns the workers' freed heap pages once the chunk is extracted.
struct TrimOnDrop;

impl Drop for TrimOnDrop {
    fn drop(&mut self) {
        crate::give_back_freed_memory();
    }
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
        let scan_like = |placement: &Placement| crate::images::is_scan_like(doc, placement);
        let mut invisible_layer = false;
        let glyphs = match outcome {
            Ok(Ok(())) => {
                let (glyphs, layer) = collector.visible_glyphs(&scan_like);
                invisible_layer = layer;
                Ok(glyphs)
            }
            Ok(Err(err)) => Err(format!("page {number}: text extraction failed ({err})")),
            Err(_) => Err(format!(
                "page {number}: text extraction failed (malformed font or content)"
            )),
        };
        let area = collector
            .media
            .map(|media| ((media.urx - media.llx) * (media.ury - media.lly)).abs())
            .filter(|area| area.is_finite())
            .unwrap_or(0.0);
        let (mut bottom, mut top) = collector
            .media
            .map(|media| (media.lly.min(media.ury), media.lly.max(media.ury)))
            .unwrap_or((0.0, 792.0));
        let glyphs = glyphs.map(|mut glyphs| {
            collector.turn_page(&mut glyphs, &mut bottom, &mut top);
            glyphs
        });
        RawPage {
            number,
            bottom,
            top,
            glyphs,
            rotated: collector.rotated,
            rules: collector.rules,
            ocr: false,
            images: collector.images,
            area,
            figures: Vec::new(),
            invisible_layer,
        }
    };
    if workers <= 1 {
        return selected.iter().map(|&number| extract_one(number)).collect();
    }
    let _trim = TrimOnDrop;
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
                rules: Vec::new(),
                ocr: false,
                images: Vec::new(),
                area: 0.0,
                figures: Vec::new(),
                invisible_layer: false,
            })
        })
        .collect()
}
