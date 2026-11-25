use pdf_extract::Document;

use crate::blocks::{layout_page, Block};
use crate::extract::extract_pages;
use crate::margins::repeated_margin_lines;
use crate::render::{heading_levels, is_size_heading, render_blocks};
use crate::rows::body_font_size;
use crate::{
    info_title, outline, ImageOptions, LayoutError, MarkdownDocument, MarkdownPage, PageImage,
};

/// Convert the selected pages (1-based; `None` = all) of a PDF to Markdown.
pub fn pdf_to_markdown(
    doc: &Document,
    pages: Option<&[u32]>,
) -> Result<MarkdownDocument, LayoutError> {
    pdf_to_markdown_with_images(doc, pages, None)
}

/// The selected pages that exist, and the page count.
fn selected_pages(doc: &Document, pages: Option<&[u32]>) -> (Vec<u32>, u32) {
    let page_map = doc.get_pages();
    let total = u32::try_from(page_map.len()).unwrap_or(u32::MAX);
    let selected: Vec<u32> = match pages {
        Some(list) => list
            .iter()
            .copied()
            .filter(|page| page_map.contains_key(page))
            .collect(),
        None => page_map.keys().copied().collect(),
    };
    (selected, total)
}

/// The embedded images of the selected pages that matter (see [`ImageOptions`]),
/// exported through `options.place`, with their page, extent and caption.
pub fn pdf_images(
    doc: &Document,
    pages: Option<&[u32]>,
    options: &ImageOptions<'_>,
) -> Vec<PageImage> {
    let (selected, _) = selected_pages(doc, pages);
    extract_pages(doc, &selected)
        .iter()
        .flat_map(|page| crate::images::figures_for_page(doc, page, options))
        .map(|(_, image)| image)
        .collect()
}

/// Like [`pdf_to_markdown`], with embedded images exported and marked where
/// they sit in reading order when `images` is given.
pub fn pdf_to_markdown_with_images(
    doc: &Document,
    pages: Option<&[u32]>,
    images: Option<&ImageOptions<'_>>,
) -> Result<MarkdownDocument, LayoutError> {
    let (selected, total) = selected_pages(doc, pages);
    let mut raw = extract_pages(doc, &selected);
    if let Some(options) = images {
        for page in &mut raw {
            page.figures = crate::images::figures_for_page(doc, page, options)
                .into_iter()
                .map(|(figure, _)| figure)
                .collect();
        }
    }
    let body_size = body_font_size(&raw);
    let repeated = repeated_margin_lines(&raw);
    let mut heading_sizes = Vec::<f64>::new();
    let mut laid_out = Vec::with_capacity(raw.len());
    for page in &raw {
        let blocks = match &page.glyphs {
            Ok(glyphs) => {
                let mut blocks = layout_page(glyphs, page, body_size, &repeated);
                if page.invisible_layer {
                    blocks.insert(
                        0,
                        Block::Comment("text layer: invisible, over page image".into()),
                    );
                }
                blocks
            }
            Err(message) => vec![Block::Comment(message.clone())],
        };
        for block in &blocks {
            if let Block::Paragraph { size, text, .. } = block {
                if is_size_heading(*size, body_size, text) {
                    heading_sizes.push(*size);
                }
            }
        }
        laid_out.push((page.number, blocks));
    }
    let levels = heading_levels(&heading_sizes);
    let mut first_heading = None;
    let pages = laid_out
        .into_iter()
        .map(|(number, blocks)| MarkdownPage {
            number,
            markdown: {
                let mut heading = None;
                let markdown = render_blocks(&blocks, body_size, &levels, &mut heading);
                if number == 1 {
                    first_heading = heading;
                }
                markdown
            },
        })
        .collect();
    let title = info_title(doc).or(first_heading);
    Ok(MarkdownDocument {
        title,
        page_count: total,
        pages,
        outline: outline(doc),
    })
}
