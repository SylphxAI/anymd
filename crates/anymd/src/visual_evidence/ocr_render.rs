use anymd_core::render::{RenderDocument, DEFAULT_MAX_RENDER_OUTPUT_BYTES};

use super::{read_pdf, render_warnings, MaterializedSource, RequestWorkBudget};

#[derive(Debug)]
pub(crate) struct RenderedOcrPage {
    pub(crate) pdf_to_pixel: [f64; 6],
    pub(crate) page: u32,
    pub(crate) png: Vec<u8>,
    pub(crate) evidence_id: String,
    pub(crate) scale: f64,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug)]
pub(crate) struct RenderedOcrSource {
    pub(crate) source_index: usize,
    pub(crate) source: String,
    pub(crate) pages: Vec<RenderedOcrPage>,
    pub(crate) warnings: Vec<String>,
    pub(crate) error: Option<String>,
}

pub(crate) struct OcrRenderRequest<'a> {
    pub(crate) requested_pages: &'a [u32],
    pub(crate) scale_value: f64,
    pub(crate) max_pages: usize,
    pub(crate) max_pixels: u64,
    pub(crate) request_deadline: std::time::Instant,
    pub(crate) deadline_error: &'a str,
}

pub(crate) fn render_ocr_source(
    source: &MaterializedSource,
    request: OcrRenderRequest<'_>,
    work_budget: &mut RequestWorkBudget,
) -> RenderedOcrSource {
    let output = (|| -> Result<(Vec<RenderedOcrPage>, Vec<String>), String> {
        work_budget.ensure_available()?;
        if std::time::Instant::now() >= request.request_deadline {
            let error = work_budget
                .exhaust(request.deadline_error.to_string())
                .expect_err("deadline exhausts render budget");
            return Err(error);
        }
        let document =
            RenderDocument::new(read_pdf(source.path())?).map_err(|error| error.message)?;
        let num_pages = document.page_count();
        if num_pages == 0 {
            return Err("PDF contains no pages.".into());
        }
        let valid = request
            .requested_pages
            .iter()
            .copied()
            .filter(|page| (*page as usize) <= num_pages)
            .collect::<Vec<_>>();
        let invalid = request
            .requested_pages
            .iter()
            .copied()
            .filter(|page| (*page as usize) > num_pages)
            .collect::<Vec<_>>();
        let pages_to_render = &valid[..valid.len().min(request.max_pages)];
        let truncated = &valid[pages_to_render.len()..];
        if pages_to_render.is_empty() {
            return Err(format!(
                "No valid OCR candidate pages for source {}.",
                source.label()
            ));
        }
        let warnings = render_warnings(&invalid, truncated, num_pages, request.max_pages);
        let scale = request.scale_value as f32;
        let mut pages = Vec::with_capacity(pages_to_render.len());
        for page in pages_to_render {
            work_budget.ensure_available()?;
            if std::time::Instant::now() >= request.request_deadline {
                let error = work_budget
                    .exhaust(request.deadline_error.to_string())
                    .expect_err("deadline exhausts render budget");
                return Err(error);
            }
            let pixels = document
                .planned_render_pixel_count(*page as usize, scale)
                .map_err(|error| error.message)?;
            work_budget.charge_page(pixels)?;
            let rendered = document
                .render_page(
                    *page as usize,
                    scale,
                    request.max_pixels,
                    DEFAULT_MAX_RENDER_OUTPUT_BYTES,
                )
                .map_err(|error| error.message)?;
            pages.push(RenderedOcrPage {
                pdf_to_pixel: rendered.pdf_to_pixel_transform(),
                page: rendered.page as u32,
                png: rendered.png,
                evidence_id: format!(
                    "page-{}-render-scale-{}",
                    rendered.page, request.scale_value
                ),
                scale: request.scale_value,
                width: rendered.width,
                height: rendered.height,
            });
        }
        Ok((pages, warnings))
    })();
    match output {
        Ok((pages, warnings)) => RenderedOcrSource {
            source_index: source.source_index(),
            source: source.label().to_string(),
            pages,
            warnings,
            error: None,
        },
        Err(error) => RenderedOcrSource {
            source_index: source.source_index(),
            source: source.label().to_string(),
            pages: Vec::new(),
            warnings: Vec::new(),
            error: Some(error),
        },
    }
}
