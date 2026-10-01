//! Document structure types produced by [`DocParser`](crate::DocParser).
//!
//! Ported from `oar-ocr-core`'s `domain::structure` and trimmed to what a VL
//! pipeline actually fills in: the classic pipeline's cell grids, per-stage
//! confidences and stitching metadata have no producer here, so they are not
//! carried as fields that would always read back empty.

use crate::geometry::BoundingBox;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A parsed page: its layout elements in reading order, plus the recognized
/// tables and formulas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureResult {
    /// Path of the source image, or `<memory>` for in-memory input.
    pub input_path: Arc<str>,
    /// Index of the page within a batch.
    pub index: usize,
    /// Detected regions, in reading order.
    pub layout_elements: Vec<LayoutElement>,
    /// Recognized tables.
    pub tables: Vec<TableResult>,
    /// Recognized formulas.
    pub formulas: Vec<FormulaResult>,
}

impl StructureResult {
    /// Creates an empty result for one page.
    pub fn new(input_path: impl Into<Arc<str>>, index: usize) -> Self {
        Self {
            input_path: input_path.into(),
            index,
            layout_elements: Vec::new(),
            tables: Vec::new(),
            formulas: Vec::new(),
        }
    }

    /// Attaches the detected layout elements.
    pub fn with_layout_elements(mut self, elements: Vec<LayoutElement>) -> Self {
        self.layout_elements = elements;
        self
    }

    /// Attaches the recognized tables.
    pub fn with_tables(mut self, tables: Vec<TableResult>) -> Self {
        self.tables = tables;
        self
    }

    /// Renders the page as Markdown with the defaults
    /// [`DocParser::parse_to_markdown`] uses: skipping
    /// [`DEFAULT_MARKDOWN_IGNORE_LABELS`] so running heads and page furniture
    /// stay out of the prose, and centring captions and tables.
    ///
    /// Call [`to_markdown`](crate::utils::to_markdown) directly to choose
    /// other labels or turn the centring off.
    ///
    /// [`DEFAULT_MARKDOWN_IGNORE_LABELS`]: crate::utils::DEFAULT_MARKDOWN_IGNORE_LABELS
    /// [`DocParser::parse_to_markdown`]: crate::DocParser::parse_to_markdown
    pub fn to_markdown(&self) -> String {
        crate::utils::to_markdown(
            &self.layout_elements,
            &crate::utils::default_markdown_ignore_labels(),
            true,
        )
    }
}

/// A layout element detected in the document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutElement {
    /// Bounding box of the element
    pub bbox: BoundingBox,
    /// Type of the layout element
    pub element_type: LayoutElementType,
    /// Confidence score for the detection
    pub confidence: f32,
    /// Optional label for the element (original model label)
    pub label: Option<String>,
    /// Optional text content for the element
    pub text: Option<String>,
    /// Reading order index (1-based).
    ///
    /// This index represents the element's position in the reading order.
    /// Only elements that should be included in reading flow (text, tables,
    /// formulas, images, etc.) will have an order index assigned.
    /// Headers, footers, and other auxiliary elements may have `None`.
    pub order_index: Option<u32>,
}

impl LayoutElement {
    /// Creates a new layout element.
    pub fn new(bbox: BoundingBox, element_type: LayoutElementType, confidence: f32) -> Self {
        Self {
            bbox,
            element_type,
            confidence,
            label: None,
            text: None,
            order_index: None,
        }
    }

    /// Sets the label for the element.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the text content for the element.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayoutElementType {
    /// Document title
    DocTitle,
    /// Paragraph/section title
    ParagraphTitle,
    /// General text content
    Text,
    /// Table of contents
    Content,
    /// Abstract section
    Abstract,

    /// Image or figure
    Image,
    /// Table
    Table,
    /// Chart or graph
    Chart,
    /// Mathematical formula
    Formula,

    /// Figure caption/title
    FigureTitle,
    /// Table caption/title
    TableTitle,
    /// Chart caption/title
    ChartTitle,
    /// Combined figure/table/chart title (PP-DocLayout)
    FigureTableChartTitle,

    /// Page header
    Header,
    /// Header image
    HeaderImage,
    /// Page footer
    Footer,
    /// Footer image
    FooterImage,
    /// Footnote
    Footnote,

    /// Stamp or official seal
    Seal,
    /// Page number
    Number,
    /// Reference section
    Reference,
    /// Reference content (PP-DocLayout_plus-L)
    ReferenceContent,
    /// Algorithm block
    Algorithm,
    /// Formula number
    FormulaNumber,
    /// Marginal/aside text
    AsideText,
    /// List items
    List,

    /// Generic document region block (PP-DocBlockLayout)
    /// Used for hierarchical layout ordering and block grouping
    Region,

    /// Other/unknown (original label preserved in LayoutElement.label)
    Other,
}

impl LayoutElementType {
    /// Returns the string representation of the element type.
    ///
    /// This returns the PP-StructureV3 compatible label string.
    pub fn as_str(&self) -> &'static str {
        match self {
            // Document Structure
            LayoutElementType::DocTitle => "doc_title",
            LayoutElementType::ParagraphTitle => "paragraph_title",
            LayoutElementType::Text => "text",
            LayoutElementType::Content => "content",
            LayoutElementType::Abstract => "abstract",

            // Visual Elements
            LayoutElementType::Image => "image",
            LayoutElementType::Table => "table",
            LayoutElementType::Chart => "chart",
            LayoutElementType::Formula => "formula",

            // Captions
            LayoutElementType::FigureTitle => "figure_title",
            LayoutElementType::TableTitle => "table_title",
            LayoutElementType::ChartTitle => "chart_title",
            LayoutElementType::FigureTableChartTitle => "figure_table_chart_title",

            // Page Structure
            LayoutElementType::Header => "header",
            LayoutElementType::HeaderImage => "header_image",
            LayoutElementType::Footer => "footer",
            LayoutElementType::FooterImage => "footer_image",
            LayoutElementType::Footnote => "footnote",

            // Special Elements
            LayoutElementType::Seal => "seal",
            LayoutElementType::Number => "number",
            LayoutElementType::Reference => "reference",
            LayoutElementType::ReferenceContent => "reference_content",
            LayoutElementType::Algorithm => "algorithm",
            LayoutElementType::FormulaNumber => "formula_number",
            LayoutElementType::AsideText => "aside_text",
            LayoutElementType::List => "list",

            // Region (PP-DocBlockLayout)
            LayoutElementType::Region => "region",

            // Fallback
            LayoutElementType::Other => "other",
        }
    }

    /// Creates a LayoutElementType from a string label with fine-grained mapping.
    ///
    /// This method maps model output labels to their corresponding fine-grained types,
    /// preserving the full PP-StructureV3 label set (20/23 classes).
    pub fn from_label(label: &str) -> Self {
        match label.to_lowercase().as_str() {
            // Document Structure
            "doc_title" => LayoutElementType::DocTitle,
            "paragraph_title" | "title" => LayoutElementType::ParagraphTitle,
            "text" | "paragraph" => LayoutElementType::Text,
            "content" => LayoutElementType::Content,
            "abstract" => LayoutElementType::Abstract,

            // Visual Elements
            "image" | "figure" => LayoutElementType::Image,
            "table" => LayoutElementType::Table,
            "chart" | "flowchart" => LayoutElementType::Chart,
            "formula" | "equation" | "display_formula" | "inline_formula" => {
                LayoutElementType::Formula
            }

            // Captions
            "figure_title" => LayoutElementType::FigureTitle,
            "table_title" => LayoutElementType::TableTitle,
            "chart_title" => LayoutElementType::ChartTitle,
            "figure_table_chart_title" | "caption" => LayoutElementType::FigureTableChartTitle,

            // Page Structure
            "header" => LayoutElementType::Header,
            "header_image" => LayoutElementType::HeaderImage,
            "footer" => LayoutElementType::Footer,
            "footer_image" => LayoutElementType::FooterImage,
            "footnote" | "vision_footnote" => LayoutElementType::Footnote,

            // Special Elements
            "seal" => LayoutElementType::Seal,
            "number" => LayoutElementType::Number,
            "reference" => LayoutElementType::Reference,
            "reference_content" => LayoutElementType::ReferenceContent,
            "algorithm" => LayoutElementType::Algorithm,
            "formula_number" => LayoutElementType::FormulaNumber,
            "aside_text" => LayoutElementType::AsideText,
            "list" => LayoutElementType::List,
            "vertical_text" => LayoutElementType::Text,

            // Region (PP-DocBlockLayout)
            "region" => LayoutElementType::Region,

            // Everything else maps to Other
            // The original label is preserved in LayoutElement.label
            _ => LayoutElementType::Other,
        }
    }

    /// Returns the semantic category for this element type.
    ///
    /// This method groups fine-grained types into broader semantic categories,
    /// useful for processing logic that doesn't need fine-grained distinctions.
    ///
    /// # Categories
    ///
    /// - **Title**: DocTitle, ParagraphTitle
    /// - **Text**: Text, Content, Abstract
    /// - **Visual**: Image, Chart
    /// - **Table**: Table
    /// - **Caption**: FigureTitle, TableTitle, ChartTitle, FigureTableChartTitle
    /// - **Header**: Header, HeaderImage
    /// - **Footer**: Footer, FooterImage, Footnote
    /// - **Formula**: Formula, FormulaNumber
    /// - **Special**: Seal, Number, Reference, ReferenceContent, Algorithm, AsideText
    /// - **List**: List
    /// - **Other**: Other
    pub fn semantic_category(&self) -> &'static str {
        match self {
            // Title category
            LayoutElementType::DocTitle | LayoutElementType::ParagraphTitle => "title",

            // Text category
            LayoutElementType::Text | LayoutElementType::Content | LayoutElementType::Abstract => {
                "text"
            }

            // Visual category
            LayoutElementType::Image | LayoutElementType::Chart => "visual",

            // Table category
            LayoutElementType::Table => "table",

            // Caption category
            LayoutElementType::FigureTitle
            | LayoutElementType::TableTitle
            | LayoutElementType::ChartTitle
            | LayoutElementType::FigureTableChartTitle => "caption",

            // Header category
            LayoutElementType::Header | LayoutElementType::HeaderImage => "header",

            // Footer category
            LayoutElementType::Footer
            | LayoutElementType::FooterImage
            | LayoutElementType::Footnote => "footer",

            // Formula category
            LayoutElementType::Formula | LayoutElementType::FormulaNumber => "formula",

            // Special category
            LayoutElementType::Seal
            | LayoutElementType::Number
            | LayoutElementType::Reference
            | LayoutElementType::ReferenceContent
            | LayoutElementType::Algorithm
            | LayoutElementType::AsideText => "special",

            // List category
            LayoutElementType::List => "list",

            // Region category (PP-DocBlockLayout)
            LayoutElementType::Region => "region",

            // Other
            LayoutElementType::Other => "other",
        }
    }

    /// Returns whether this element type is a title variant.
    pub fn is_title(&self) -> bool {
        matches!(
            self,
            LayoutElementType::DocTitle | LayoutElementType::ParagraphTitle
        )
    }

    /// Returns whether this element type is a visual element (image, chart, figure).
    pub fn is_visual(&self) -> bool {
        matches!(self, LayoutElementType::Image | LayoutElementType::Chart)
    }

    /// Returns whether this element type is a caption variant.
    pub fn is_caption(&self) -> bool {
        matches!(
            self,
            LayoutElementType::FigureTitle
                | LayoutElementType::TableTitle
                | LayoutElementType::ChartTitle
                | LayoutElementType::FigureTableChartTitle
        )
    }

    /// Returns whether this element type is a header variant.
    pub fn is_header(&self) -> bool {
        matches!(
            self,
            LayoutElementType::Header | LayoutElementType::HeaderImage
        )
    }

    /// Returns whether this element type is a footer variant.
    pub fn is_footer(&self) -> bool {
        matches!(
            self,
            LayoutElementType::Footer
                | LayoutElementType::FooterImage
                | LayoutElementType::Footnote
        )
    }

    /// Returns whether this element type is a formula variant.
    pub fn is_formula(&self) -> bool {
        matches!(
            self,
            LayoutElementType::Formula | LayoutElementType::FormulaNumber
        )
    }

    /// Returns whether this element type contains text content that should be OCR'd.
    pub fn should_ocr(&self) -> bool {
        matches!(
            self,
            LayoutElementType::Text
                | LayoutElementType::Content
                | LayoutElementType::Abstract
                | LayoutElementType::DocTitle
                | LayoutElementType::ParagraphTitle
                | LayoutElementType::FigureTitle
                | LayoutElementType::TableTitle
                | LayoutElementType::ChartTitle
                | LayoutElementType::FigureTableChartTitle
                | LayoutElementType::Header
                | LayoutElementType::HeaderImage
                | LayoutElementType::Footer
                | LayoutElementType::FooterImage
                | LayoutElementType::Footnote
                | LayoutElementType::Reference
                | LayoutElementType::ReferenceContent
                | LayoutElementType::Algorithm
                | LayoutElementType::AsideText
                | LayoutElementType::List
                | LayoutElementType::Number
        )
    }
}

/// Result of table recognition.
///
/// A VL backend recognizes a table region in one shot and returns HTML, so
/// there is no cell grid or per-stage confidence here — the classic pipeline's
/// cell detection, structure tokens and stitching all live in `oar-ocr-core`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableResult {
    /// Bounding box of the table in the original image
    pub bbox: BoundingBox,
    /// Table type (wired or wireless)
    pub table_type: TableType,
    /// HTML structure of the table (if available)
    pub html_structure: Option<String>,
}

impl TableResult {
    /// Creates a new table result.
    pub fn new(bbox: BoundingBox, table_type: TableType) -> Self {
        Self {
            bbox,
            table_type,
            html_structure: None,
        }
    }

    /// Sets the HTML structure.
    pub fn with_html_structure(mut self, html: impl Into<String>) -> Self {
        self.html_structure = Some(html.into());
        self
    }

    /// Returns true if this table has recognized structure.
    pub fn has_structure(&self) -> bool {
        self.html_structure.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableType {
    /// Table with visible borders
    Wired,
    /// Table without visible borders
    Wireless,
    /// Unknown table type
    Unknown,
}

/// Result of formula recognition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormulaResult {
    /// Bounding box of the formula in the original image
    pub bbox: BoundingBox,
    /// LaTeX representation of the formula
    pub latex: String,
    /// Confidence score for the recognition
    pub confidence: f32,
}

impl FormulaResult {
    /// Creates a new formula result.
    pub fn new(bbox: BoundingBox, latex: impl Into<String>, confidence: f32) -> Self {
        Self {
            bbox,
            latex: latex.into(),
            confidence,
        }
    }
}
