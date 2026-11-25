use pdf_extract::Document;

/// A parsed PDF (lopdf document).
pub type PdfDocument = Document;

/// One page of converted Markdown.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownPage {
    /// 1-based page number.
    pub number: u32,
    pub markdown: String,
}

/// A converted PDF (or the requested subset of its pages).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownDocument {
    pub title: Option<String>,
    pub page_count: u32,
    pub pages: Vec<MarkdownPage>,
    /// Bookmarks as (depth, title, page), depth 0 = top level.
    pub outline: Vec<(usize, String, Option<u32>)>,
}
