//! Literal PDF text indexing and search for pdf-reader-mcp.

use crate::pdfjs_text::decode_pdfjs_text_string;
use std::collections::BTreeMap;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use pdf_extract::{
    output_doc, ColorSpace, Document, MediaBox, Object, OutputDev, OutputError, Path as PdfPath,
    Transform,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// Per-page extraction budgets: each page gets its own allowance, so dense
// multi-page documents are not rejected just because their cumulative text
// is large. A separate document-wide backstop below still bounds total work.
const MAX_EXTRACTED_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_GEOMETRY_CHARS: usize = 250_000;
// Document-wide backstop across all pages of one extraction request: 8x the
// per-page allowance, enough for ~1000 pages of average density. Keeps
// pathological documents fail-closed without capping ordinary large PDFs.
const MAX_TOTAL_EXTRACTED_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_GEOMETRY_CHARS: usize = 2_000_000;
const MAX_RAW_TEXT_PARTS: usize = 65_536;
const MAX_RAW_TEXT_PARTS_PER_PAGE: usize = 8_192;
const MAX_NORMALIZED_TEXT_SEGMENTS: usize = 65_536;
const MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE: usize = 8_192;
const TEXT_SEGMENT_GAP_THRESHOLD: f64 = 48.0;

pub const TEXT_INDEX_ROUTE: &str = "rust-text-index";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextBoundingBox {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

impl TextBoundingBox {
    fn from_character(trm: &Transform, width: f64, font_size: f64) -> Option<Self> {
        let values = [
            trm.m11, trm.m12, trm.m21, trm.m22, trm.m31, trm.m32, width, font_size,
        ];
        if !values.into_iter().all(f64::is_finite) {
            return None;
        }
        let x_advance = (trm.m11 * width * font_size).abs();
        let y_advance = (trm.m12 * width * font_size).abs();
        let glyph_width = x_advance.hypot(y_advance);
        let x_height = (trm.m21 * font_size).abs();
        let y_height = (trm.m22 * font_size).abs();
        let glyph_height = x_height.hypot(y_height);
        let right = trm.m31 + glyph_width.max(0.0);
        let top = trm.m32 + glyph_height.max(0.0);
        Some(Self {
            left: canonical_coordinate(trm.m31)?,
            bottom: canonical_coordinate(trm.m32)?,
            right: canonical_coordinate(right)?,
            top: canonical_coordinate(top)?,
        })
    }

    fn union(self, other: Self) -> Option<Self> {
        let union = Self {
            left: self.left.min(other.left),
            bottom: self.bottom.min(other.bottom),
            right: self.right.max(other.right),
            top: self.top.max(other.top),
        };
        let width = union.right - union.left;
        let height = union.top - union.bottom;
        [
            union.left,
            union.bottom,
            union.right,
            union.top,
            width,
            height,
        ]
        .into_iter()
        .all(f64::is_finite)
        .then_some(union)
    }

    fn estimated_utf16_range(self, text_len: u32, start: u32, end: u32) -> Option<Self> {
        let width = self.right - self.left;
        if text_len == 0 || end <= start || self.right <= self.left || !width.is_finite() {
            return None;
        }
        let start_ratio = f64::from(start.min(text_len)) / f64::from(text_len);
        let end_ratio = f64::from(end.min(text_len).max(start)) / f64::from(text_len);
        let estimated = Self {
            left: canonical_zero(self.left + width * start_ratio),
            bottom: self.bottom,
            right: canonical_zero(self.left + width * end_ratio),
            top: self.top,
        };
        [
            estimated.left,
            estimated.bottom,
            estimated.right,
            estimated.top,
        ]
        .into_iter()
        .all(f64::is_finite)
        .then_some(estimated)
    }
}

fn canonical_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn canonical_coordinate(value: f64) -> Option<f64> {
    const SCALE: f64 = 10_000.0;
    let scaled = value * SCALE;
    scaled
        .is_finite()
        .then(|| canonical_zero(scaled.round() / SCALE))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextCharacterGeometry {
    pub text: String,
    pub item_char_start: u32,
    pub item_char_end: u32,
    pub is_whitespace: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<TextBoundingBox>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionedTextRun {
    pub text: String,
    pub item_char_start: u32,
    pub item_char_end: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<TextBoundingBox>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionedTextItem {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<TextBoundingBox>,
    pub chars: Vec<TextCharacterGeometry>,
    pub runs: Vec<PositionedTextRun>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSearchMatch {
    pub id: String,
    pub page: u32,
    pub text: String,
    pub snippet: String,
    pub match_start: u32,
    pub match_end: u32,
    pub text_item_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<TextBoundingBox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box_level: Option<String>,
    pub route: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedPageText {
    pub text: String,
    pub items: Vec<String>,
    pub positioned_items: Vec<PositionedTextItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfInfo {
    pub format_version: String,
    /// Document Info dictionary fields projected like pdf.js getMetadata().info
    /// (standard string keys, Trapped Name object, and nested Custom map).
    pub fields: BTreeMap<String, Value>,
    /// Catalog /Lang; `None` serializes as JSON null like pdf.js.
    pub language: Option<String>,
    /// Encrypt dictionary Filter name; `None` serializes as JSON null like pdf.js.
    pub encrypt_filter_name: Option<String>,
    pub is_linearized: bool,
    pub is_acroform_present: bool,
    pub is_xfa_present: bool,
    pub is_collection_present: bool,
    pub is_signatures_present: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedPdfText {
    pub pages: Vec<ExtractedPageText>,
    pub info: PdfInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSearchResult {
    pub num_pages: u32,
    pub searched_pages: Vec<u32>,
    pub total_matches: u32,
    pub matches: Vec<TextSearchMatch>,
    pub route: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_cache: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextIndexErrorCode {
    InvalidParams,
    InvalidRequest,
    ExtractionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextIndexError {
    pub code: TextIndexErrorCode,
    pub message: String,
}

impl From<crate::HashError> for TextIndexError {
    fn from(error: crate::HashError) -> Self {
        match error.code {
            crate::HashErrorCode::InvalidParams => Self::invalid_params(error.message),
            crate::HashErrorCode::InvalidRequest => Self::invalid_request(error.message),
        }
    }
}

impl TextIndexError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: TextIndexErrorCode::InvalidParams,
            message: message.into(),
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: TextIndexErrorCode::InvalidRequest,
            message: message.into(),
        }
    }

    pub(crate) fn extraction_failed(message: impl Into<String>) -> Self {
        Self {
            code: TextIndexErrorCode::ExtractionFailed,
            message: message.into(),
        }
    }
}

fn validate_pdf_path(path: &Path, max_file_bytes: u64) -> Result<(), TextIndexError> {
    let meta = fs::metadata(path).map_err(|err| {
        TextIndexError::invalid_request(format!(
            "Unable to access file at '{}': {err}",
            path.display()
        ))
    })?;

    if !meta.is_file() {
        return Err(TextIndexError::invalid_request(format!(
            "Path '{}' is not a regular file.",
            path.display()
        )));
    }

    if meta.len() > max_file_bytes {
        return Err(TextIndexError::invalid_request(format!(
            "File exceeds maximum size of {} bytes.",
            max_file_bytes
        )));
    }

    Ok(())
}

/// A glyph's extent along the text direction and its font size, kept beside
/// each character so word spaces can be inferred from real glyph gaps.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GlyphExtent {
    x0: f64,
    x1: f64,
    size: f64,
}

impl GlyphExtent {
    /// Same projection as the anymd-pdf layout engine: start along the text
    /// direction, advance including character spacing (Tc) but not word
    /// spacing, and a half-em fallback for fonts without widths.
    fn from_character(
        trm: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        character: &str,
    ) -> Option<Self> {
        let values = [
            trm.m11, trm.m12, trm.m21, trm.m22, trm.m31, trm.m32, width, spacing, font_size,
        ];
        if !values.into_iter().all(f64::is_finite) {
            return None;
        }
        let size = (trm.m21.hypot(trm.m22) * font_size).abs();
        let scale = trm.m11.hypot(trm.m12);
        if size <= 0.1 || scale <= 0.0 {
            return None;
        }
        let (dx, dy) = (trm.m11 / scale, trm.m12 / scale);
        let tracking = if character.chars().all(char::is_whitespace) {
            0.0
        } else {
            spacing
        };
        let mut advance = ((width * font_size + tracking) * scale).abs();
        if advance < size * 0.05 {
            advance = size * 0.5;
        }
        let along = if dx > 0.9 && dy.abs() < 0.2 {
            trm.m31
        } else {
            trm.m31 * dx + trm.m32 * dy
        };
        let extent = Self {
            x0: along,
            x1: along + advance,
            size,
        };
        [extent.x0, extent.x1]
            .into_iter()
            .all(f64::is_finite)
            .then_some(extent)
    }
}

#[derive(Debug)]
struct RawTextPart {
    item: PositionedTextItem,
    /// One entry per `item.chars` entry.
    glyphs: Vec<Option<GlyphExtent>>,
    x: Option<f64>,
    y: Option<f64>,
    right: Option<f64>,
}

impl RawTextPart {
    fn empty() -> Self {
        Self {
            item: PositionedTextItem {
                text: String::new(),
                bounding_box: None,
                chars: Vec::new(),
                runs: Vec::new(),
            },
            glyphs: Vec::new(),
            x: None,
            y: None,
            right: None,
        }
    }
}

#[derive(Default)]
struct TextItemOutput {
    pages: Vec<Vec<RawTextPart>>,
    current_part: Option<RawTextPart>,
    current_item_utf16_len: u32,
    current_item_geometry_valid: bool,
    text_bytes: usize,
    geometry_chars: usize,
    total_text_bytes: usize,
    total_geometry_chars: usize,
    raw_part_count: usize,
    page_raw_part_count: usize,
}

impl TextItemOutput {
    fn finish_part(&mut self) {
        let Some(mut part) = self.current_part.take() else {
            return;
        };
        self.current_item_utf16_len = 0;
        if !self.current_item_geometry_valid {
            part.item.bounding_box = None;
            for character in &mut part.item.chars {
                character.bounding_box = None;
            }
        } else if let Some(item_box) = part.item.bounding_box {
            let text_len = part
                .item
                .text
                .encode_utf16()
                .count()
                .try_into()
                .unwrap_or(u32::MAX);
            for character in &mut part.item.chars {
                character.bounding_box = item_box.estimated_utf16_range(
                    text_len,
                    character.item_char_start,
                    character.item_char_end,
                );
            }
        }
        self.current_item_geometry_valid = false;
        if !part.item.text.is_empty() {
            if let Some(page) = self.pages.last_mut() {
                page.push(part);
            }
        }
    }

    fn admit_part(&mut self) -> Result<(), OutputError> {
        if self.raw_part_count >= MAX_RAW_TEXT_PARTS
            || self.page_raw_part_count >= MAX_RAW_TEXT_PARTS_PER_PAGE
        {
            return Err(OutputError::IoError(std::io::Error::other(
                "selectable text exceeds bounded raw-part budget",
            )));
        }
        self.raw_part_count += 1;
        self.page_raw_part_count += 1;
        Ok(())
    }
}

impl OutputDev for TextItemOutput {
    fn begin_page(
        &mut self,
        _page_num: u32,
        _media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.finish_part();
        self.pages.push(Vec::new());
        self.page_raw_part_count = 0;
        self.text_bytes = 0;
        self.geometry_chars = 0;
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.finish_part();
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
        self.text_bytes = self.text_bytes.saturating_add(character.len());
        self.geometry_chars = self.geometry_chars.saturating_add(1);
        self.total_text_bytes = self.total_text_bytes.saturating_add(character.len());
        self.total_geometry_chars = self.total_geometry_chars.saturating_add(1);
        if self.text_bytes > MAX_EXTRACTED_TEXT_BYTES || self.geometry_chars > MAX_GEOMETRY_CHARS {
            return Err(OutputError::IoError(std::io::Error::other(
                "selectable text exceeds bounded extraction budget",
            )));
        }
        if self.total_text_bytes > MAX_TOTAL_EXTRACTED_TEXT_BYTES
            || self.total_geometry_chars > MAX_TOTAL_GEOMETRY_CHARS
        {
            return Err(OutputError::IoError(std::io::Error::other(
                "selectable text exceeds bounded document extraction budget",
            )));
        }
        if let Some(part) = self.current_part.as_mut() {
            if part.x.is_none() {
                part.x = Some(trm.m31);
                part.y = Some(trm.m32);
            }
            let advance_right = trm.m31 + trm.m11 * (width * font_size + spacing);
            if advance_right.is_finite() {
                part.right = Some(
                    part.right
                        .map_or(advance_right, |right| right.max(advance_right)),
                );
            }
            let start = self.current_item_utf16_len;
            part.item.text.push_str(character);
            let char_units = character
                .encode_utf16()
                .count()
                .try_into()
                .unwrap_or(u32::MAX);
            let end = start.saturating_add(char_units);
            self.current_item_utf16_len = end;
            let mut bounding_box = if self.current_item_geometry_valid {
                TextBoundingBox::from_character(trm, width, font_size)
            } else {
                None
            };
            if self.current_item_geometry_valid && bounding_box.is_none() {
                self.current_item_geometry_valid = false;
                part.item.bounding_box = None;
                for existing in &mut part.item.chars {
                    existing.bounding_box = None;
                }
            }
            if let Some(box_) = bounding_box {
                if let Some(current) = part.item.bounding_box {
                    if let Some(union) = current.union(box_) {
                        part.item.bounding_box = Some(union);
                    } else {
                        self.current_item_geometry_valid = false;
                        part.item.bounding_box = None;
                        for existing in &mut part.item.chars {
                            existing.bounding_box = None;
                        }
                        bounding_box = None;
                    }
                } else {
                    part.item.bounding_box = Some(box_);
                }
            }
            part.item.chars.push(TextCharacterGeometry {
                text: character.to_string(),
                item_char_start: start,
                item_char_end: end,
                is_whitespace: character.chars().all(char::is_whitespace),
                bounding_box,
            });
            part.glyphs.push(GlyphExtent::from_character(
                trm, width, spacing, font_size, character,
            ));
        }
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.finish_part();
        self.admit_part()?;
        self.current_part = Some(RawTextPart::empty());
        self.current_item_geometry_valid = true;
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        self.finish_part();
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        self.finish_part();
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

fn normalized_row_key(y: f64) -> Result<i64, TextIndexError> {
    if !y.is_finite() {
        return Err(TextIndexError::extraction_failed(
            "selectable text contains a non-finite row coordinate",
        ));
    }
    // JavaScript Math.round(x) is floor(x + 0.5), including for negative halves.
    let rounded = (y + 0.5).floor();
    if rounded < i64::MIN as f64 || rounded > i64::MAX as f64 {
        return Err(TextIndexError::extraction_failed(
            "selectable text row coordinate exceeds the supported range",
        ));
    }
    Ok(rounded as i64)
}

fn offset_overflow() -> TextIndexError {
    TextIndexError::extraction_failed("selectable text offset overflow")
}

/// For the characters of one segment in reading order, whether a synthetic
/// word space belongs before each one.
///
/// pdf-extract reports each show-text string as its own part and never emits
/// the word spaces that TeX and similar producers express only as glyph
/// positioning, so the text would read "Thedominantsequence...". The gaps
/// between glyphs decide instead, with the same rules as the anymd-pdf
/// layout engine behind the Markdown path.
fn segment_word_spaces(parts: &[RawTextPart]) -> Vec<bool> {
    let glyphs = parts
        .iter()
        .flat_map(|part| {
            part.item
                .chars
                .iter()
                .enumerate()
                .map(|(index, character)| {
                    let extent = part.glyphs.get(index).copied().flatten();
                    anymd_pdf::SpacingGlyph {
                        x0: extent.map_or(f64::NAN, |extent| extent.x0),
                        x1: extent.map_or(f64::NAN, |extent| extent.x1),
                        size: extent.map_or(f64::NAN, |extent| extent.size),
                        text: character.text.as_str(),
                    }
                })
        })
        .collect::<Vec<_>>();
    anymd_pdf::infer_word_spaces(&glyphs)
}

fn merge_raw_text_segment(parts: Vec<RawTextPart>) -> Result<PositionedTextItem, TextIndexError> {
    let spaces_before = segment_word_spaces(&parts);
    let mut spaces_before = spaces_before.into_iter();
    let mut text = String::new();
    let mut chars = Vec::new();
    let mut runs: Vec<PositionedTextRun> = Vec::with_capacity(parts.len());
    let mut bounding_box = None;
    let mut offset = 0u32;

    for part in parts {
        let mut run_start = offset;
        let mut run_text = String::with_capacity(part.item.text.len());
        for (index, mut character) in part.item.chars.into_iter().enumerate() {
            if spaces_before.next().unwrap_or(false) {
                // A space before a part's first glyph ends the previous run,
                // so runs stay contiguous and each keeps its own source text.
                let space_start = offset;
                offset = offset.checked_add(1).ok_or_else(offset_overflow)?;
                text.push(' ');
                chars.push(TextCharacterGeometry {
                    text: " ".to_string(),
                    item_char_start: space_start,
                    item_char_end: offset,
                    is_whitespace: true,
                    bounding_box: None,
                });
                match runs.last_mut() {
                    Some(previous) if index == 0 => {
                        previous.text.push(' ');
                        previous.item_char_end = offset;
                        run_start = offset;
                    }
                    _ => run_text.push(' '),
                }
            }
            let len = character
                .item_char_end
                .checked_sub(character.item_char_start)
                .ok_or_else(|| {
                    TextIndexError::extraction_failed("selectable text character offset overflow")
                })?;
            character.item_char_start = offset;
            offset = offset.checked_add(len).ok_or_else(offset_overflow)?;
            character.item_char_end = offset;
            text.push_str(&character.text);
            run_text.push_str(&character.text);
            chars.push(character);
        }
        if let Some(box_) = part.item.bounding_box {
            bounding_box = match bounding_box {
                None => Some(box_),
                Some(current) => Some(current.union(box_).ok_or_else(|| {
                    TextIndexError::extraction_failed("selectable text bounding-box overflow")
                })?),
            };
        }
        runs.push(PositionedTextRun {
            text: run_text,
            item_char_start: run_start,
            item_char_end: offset,
            bounding_box: part.item.bounding_box,
        });
    }

    Ok(PositionedTextItem {
        text,
        bounding_box,
        chars,
        runs,
    })
}

fn normalize_page_text_parts(
    parts: Vec<RawTextPart>,
    request_segment_count: &mut usize,
) -> Result<Vec<PositionedTextItem>, TextIndexError> {
    let mut rows = BTreeMap::<i64, Vec<(usize, RawTextPart)>>::new();
    for (source_index, part) in parts.into_iter().enumerate() {
        let x = part.x.filter(|value| value.is_finite()).ok_or_else(|| {
            TextIndexError::extraction_failed("selectable text contains an invalid X coordinate")
        })?;
        let y = part.y.ok_or_else(|| {
            TextIndexError::extraction_failed("selectable text contains a missing Y coordinate")
        })?;
        let right = part
            .right
            .or_else(|| part.item.bounding_box.map(|box_| box_.right))
            .filter(|value| value.is_finite() && *value >= x)
            .ok_or_else(|| {
                TextIndexError::extraction_failed(
                    "selectable text contains an invalid horizontal advance",
                )
            })?;
        let key = normalized_row_key(y)?;
        let mut part = part;
        part.x = Some(x);
        part.right = Some(right);
        rows.entry(key).or_default().push((source_index, part));
    }

    let mut output = Vec::new();
    for (_, mut row) in rows.into_iter().rev() {
        row.sort_by(|(left_index, left), (right_index, right)| {
            left.x
                .unwrap_or_default()
                .total_cmp(&right.x.unwrap_or_default())
                .then_with(|| left_index.cmp(right_index))
        });
        let mut current = Vec::new();
        let mut previous_right: Option<f64> = None;
        for (_, part) in row {
            let x = part.x.expect("validated text-part X");
            if previous_right.is_some_and(|right| x - right > TEXT_SEGMENT_GAP_THRESHOLD) {
                if output.len() >= MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE
                    || *request_segment_count >= MAX_NORMALIZED_TEXT_SEGMENTS
                {
                    return Err(TextIndexError::extraction_failed(
                        "selectable text exceeds bounded normalized-segment budget",
                    ));
                }
                output.push(merge_raw_text_segment(std::mem::take(&mut current))?);
                *request_segment_count += 1;
            }
            previous_right = Some(previous_right.map_or(
                part.right.expect("validated text-part right edge"),
                |right| right.max(part.right.expect("validated text-part right edge")),
            ));
            current.push(part);
        }
        if !current.is_empty() {
            if output.len() >= MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE
                || *request_segment_count >= MAX_NORMALIZED_TEXT_SEGMENTS
            {
                return Err(TextIndexError::extraction_failed(
                    "selectable text exceeds bounded normalized-segment budget",
                ));
            }
            output.push(merge_raw_text_segment(current)?);
            *request_segment_count += 1;
        }
    }
    Ok(output)
}

fn xfa_present(doc: &Document, value: &Object) -> bool {
    match value {
        Object::Array(items) => !items.is_empty(),
        Object::Reference(id) => doc
            .get_object(*id)
            .ok()
            .is_some_and(|obj| xfa_present(doc, obj)),
        Object::Stream(stream) => !stream.content.is_empty(),
        _ => false,
    }
}

fn has_only_document_signatures(doc: &Document, fields: &[Object], depth: usize) -> bool {
    const RECURSION_LIMIT: usize = 10;
    if depth > RECURSION_LIMIT || fields.is_empty() {
        return false;
    }
    fields.iter().all(|field| {
        let resolved = match field {
            Object::Reference(id) => doc.get_object(*id).ok(),
            other => Some(other),
        };
        let Some(Object::Dictionary(dict)) = resolved else {
            return false;
        };
        if let Some(kids) = dict.get(b"Kids").ok().and_then(|value| match value {
            Object::Reference(id) => doc.get_object(*id).ok().and_then(|obj| match obj {
                Object::Array(arr) => Some(arr.clone()),
                _ => None,
            }),
            Object::Array(arr) => Some(arr.clone()),
            _ => None,
        }) {
            return has_only_document_signatures(doc, &kids, depth + 1);
        }
        let is_signature = dict
            .get(b"FT")
            .ok()
            .and_then(|value| value.as_name().ok())
            .is_some_and(|name| name == b"Sig");
        let is_invisible = dict
            .get(b"Rect")
            .ok()
            .and_then(|value| match value {
                Object::Reference(id) => doc.get_object(*id).ok().and_then(|obj| match obj {
                    Object::Array(arr) => Some(arr.clone()),
                    _ => None,
                }),
                Object::Array(arr) => Some(arr.clone()),
                _ => None,
            })
            .is_some_and(|rect| {
                !rect.is_empty()
                    && rect.iter().all(|entry| match entry {
                        Object::Integer(0) => true,
                        Object::Real(v) => *v == 0.0,
                        Object::Reference(id) => doc.get_object(*id).ok().is_some_and(|obj| {
                            matches!(obj, Object::Integer(0))
                                || matches!(obj, Object::Real(v) if *v == 0.0)
                        }),
                        _ => false,
                    })
            });
        is_signature && is_invisible
    })
}

pub(crate) fn read_pdf_info(doc: &Document) -> PdfInfo {
    let mut fields = BTreeMap::new();
    let info_object = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|object| match object {
            Object::Reference(id) => doc.get_object(*id).ok(),
            Object::Dictionary(_) => Some(object),
            _ => None,
        });
    if let Some(info) = info_object.and_then(|object| object.as_dict().ok()) {
        const STANDARD_STRING_KEYS: &[&[u8]] = &[
            b"Title",
            b"Author",
            b"Subject",
            b"Keywords",
            b"Creator",
            b"Producer",
            b"CreationDate",
            b"ModDate",
        ];
        let mut custom = serde_json::Map::new();
        for (key_bytes, value) in info.iter() {
            let Ok(key) = std::str::from_utf8(key_bytes) else {
                continue;
            };
            if STANDARD_STRING_KEYS
                .iter()
                .any(|candidate| *candidate == key_bytes)
            {
                if let Some(decoded) = decode_pdfjs_text_string(value) {
                    fields.insert(key.to_string(), Value::String(decoded));
                }
                continue;
            }
            if key_bytes == b"Trapped" {
                // pdf.js: only Name values are admitted for Trapped.
                if let Ok(name) = value.as_name() {
                    if let Ok(name) = std::str::from_utf8(name) {
                        fields.insert("Trapped".into(), json!({ "name": name }));
                    }
                }
                continue;
            }
            // pdf.js default branch: string/number/boolean/Name become Custom[key].
            let custom_value = match value {
                Object::String(_, _) => decode_pdfjs_text_string(value).map(Value::String),
                Object::Integer(n) => Some(json!(*n)),
                Object::Real(n) if n.is_finite() => Some(json!(f64::from(*n))),
                Object::Boolean(b) => Some(json!(*b)),
                Object::Name(name) => std::str::from_utf8(name)
                    .ok()
                    .map(|name| json!({ "name": name })),
                _ => None,
            };
            if let Some(custom_value) = custom_value {
                custom.insert(key.to_string(), custom_value);
            }
        }
        if !custom.is_empty() {
            fields.insert("Custom".into(), Value::Object(custom));
        }
    }
    let catalog = doc.catalog().ok();
    let language = catalog.and_then(|catalog| {
        catalog
            .get(b"Lang")
            .ok()
            .and_then(|value| decode_pdfjs_text_string(value))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    });
    let encrypt_filter_name = doc
        .trailer
        .get(b"Encrypt")
        .ok()
        .and_then(|value| match value {
            Object::Reference(id) => doc.get_object(*id).ok(),
            Object::Dictionary(_) => Some(value),
            _ => None,
        })
        .and_then(|value| value.as_dict().ok())
        .and_then(|dict| dict.get(b"Filter").ok())
        .and_then(|value| value.as_name().ok())
        .and_then(|value| std::str::from_utf8(value).ok())
        .map(|value| value.to_string())
        .filter(|value| !value.is_empty());
    // pdf.js IsLinearized requires first-object Linearization.create validation
    // against the exact source stream length. Document object graphs alone are
    // insufficient and over-admit; callers with source bytes should override.
    let is_linearized = false;
    let acroform = catalog.and_then(|catalog| {
        catalog.get(b"AcroForm").ok().and_then(|value| match value {
            Object::Reference(id) => doc.get_object(*id).ok(),
            Object::Dictionary(_) => Some(value),
            _ => None,
        })
    });
    let acroform_dict = acroform.and_then(|value| value.as_dict().ok());
    // Match pdf.js formInfo:
    // hasFields = Fields is a non-empty array
    // hasXfa = XFA array non-empty OR non-empty stream
    // hasSignatures = SigFlags & 0x1
    // hasOnlyDocumentSignatures = every leaf field is invisible Sig
    // hasAcroForm = hasFields && !hasOnlyDocumentSignatures
    let fields_obj = acroform_dict.and_then(|dict| dict.get(b"Fields").ok());
    let fields_array = fields_obj.and_then(|value| match value {
        Object::Reference(id) => doc.get_object(*id).ok().and_then(|obj| obj.as_array().ok()),
        Object::Array(arr) => Some(arr),
        _ => None,
    });
    let has_fields = fields_array.is_some_and(|arr| !arr.is_empty());
    let is_xfa_present = acroform_dict
        .and_then(|dict| dict.get(b"XFA").ok())
        .is_some_and(|value| xfa_present(doc, value));
    let is_collection_present = catalog
        .and_then(|catalog| catalog.get(b"Collection").ok())
        .is_some();
    let is_signatures_present = acroform_dict
        .and_then(|dict| dict.get(b"SigFlags").ok())
        .and_then(|value| value.as_i64().ok())
        .is_some_and(|flags| flags & 1 != 0);
    let has_only_document_signatures = is_signatures_present
        && fields_array.is_some_and(|arr| has_only_document_signatures(doc, arr, 0));
    let is_acroform_present = has_fields && !has_only_document_signatures;

    PdfInfo {
        format_version: doc.version.clone(),
        fields,
        language,
        encrypt_filter_name,
        is_linearized,
        is_acroform_present,
        is_xfa_present,
        is_collection_present,
        is_signatures_present,
    }
}

pub fn extract_pdf_text(
    path: &Path,
    max_file_bytes: u64,
) -> Result<ExtractedPdfText, TextIndexError> {
    validate_pdf_path(path, max_file_bytes)?;

    let mut doc = Document::load(path).map_err(|err| {
        TextIndexError::extraction_failed(format!("Failed to extract PDF text: {err}"))
    })?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|err| {
            TextIndexError::extraction_failed(format!("Failed to extract PDF text: {err}"))
        })?;
    }
    extract_pdf_text_from_document(&doc)
}

pub(crate) fn extract_pdf_text_from_document(
    doc: &Document,
) -> Result<ExtractedPdfText, TextIndexError> {
    let info = read_pdf_info(doc);
    extract_output_bounded(doc, info)
}

/// `pdf-extract` transitively unwraps malformed-font parsing results (including
/// CFF Custom encodings), so an upstream panic must be contained at this
/// extraction boundary and reported like any other extraction failure.
///
/// The same boundary also pre-validates content streams before `output_doc`
/// runs: lopdf 0.42's inline-image parser `unwrap()`s a missing `/CS` entry
/// (`parser/mod.rs`, `image_data_stream`), so a `BI ... ID ... EI` construct
/// without a colorspace (and not an image mask) panics inside
/// `Content::decode` instead of returning `Parse(InvalidContentStream)`
/// (SylphxAI/pdf-reader-mcp#675). Pre-validation computes the expected inline
/// image data length from the inline dict (`/W`, `/H`, `/BPC`, `/CS`, with
/// `/Width`, `/Height`, `/BitsPerComponent`, `/ColorSpace` and `/IM`,
/// `/ImageMask`, `/G`, `/RGB`, `/CMYK` abbreviations supported) and verifies
/// `EI` follows, rather than scanning for a literal `EI` that can match
/// inside image data. Any page whose content stream fails validation is
/// reported as a page-level tool error naming the page, never a panic.
fn extract_output_bounded(
    doc: &Document,
    info: PdfInfo,
) -> Result<ExtractedPdfText, TextIndexError> {
    validate_page_content_streams(doc)?;
    let mut output = TextItemOutput::default();
    // Defense in depth: pre-validation above rejects the known malformed
    // inline-image shapes, but any other upstream parse panic (CFF fonts,
    // future lopdf paths) must still surface as a structured tool error.
    catch_unwind(AssertUnwindSafe(|| output_doc(doc, &mut output)))
        .map_err(|_| {
            TextIndexError::extraction_failed("Failed to extract PDF text: malformed font encoding")
        })?
        .map_err(|err| {
            TextIndexError::extraction_failed(format!("Failed to extract PDF text: {err}"))
        })?;

    let pages = if output.pages.is_empty() {
        vec![ExtractedPageText {
            text: String::new(),
            items: Vec::new(),
            positioned_items: Vec::new(),
        }]
    } else {
        let mut request_segment_count = 0usize;
        output
            .pages
            .into_iter()
            .map(|parts| {
                let positioned_items =
                    normalize_page_text_parts(parts, &mut request_segment_count)?;
                let items = positioned_items
                    .iter()
                    .map(|item| item.text.clone())
                    .collect::<Vec<_>>();
                // One line per positioned item, like the document text layer;
                // concatenating them glued words across line ends.
                Ok(ExtractedPageText {
                    text: items.join("\n"),
                    items,
                    positioned_items,
                })
            })
            .collect::<Result<Vec<_>, TextIndexError>>()?
    };

    Ok(ExtractedPdfText { pages, info })
}

/// Pre-validate every page content stream before `output_doc` parses it.
///
/// lopdf's content parser panics (rather than returning
/// `Parse(InvalidContentStream)`) on inline images whose dict cannot yield an
/// image length — notably a missing `/CS` entry when `/IM` is not true, which
/// `unwrap()`s a `DictKey("ColorSpace")` error. A panic inside
/// `Content::decode` would escape as a worker abort; instead each page is
/// decoded defensively and inline-image dicts are length-checked up front so
/// the failure below becomes `Failed to extract PDF text: invalid content
/// stream (page N)`.
fn validate_page_content_streams(doc: &Document) -> Result<(), TextIndexError> {
    for (page_number, object_id) in doc.get_pages() {
        let content = doc.get_page_content(object_id).map_err(|err| {
            TextIndexError::extraction_failed(format!(
                "Failed to extract PDF text: invalid content stream (page {page_number}): {err}"
            ))
        })?;
        validate_inline_images(&content).map_err(|detail| {
            TextIndexError::extraction_failed(format!(
                "Failed to extract PDF text: invalid content stream (page {page_number}): {detail}"
            ))
        })?;
        // `Content::decode` itself panics on the shapes above (the `cut`
        // combinator turns the `unwrap()` into a `Failure`, which the
        // non-strict entry point still propagates by panic). Run it under
        // `catch_unwind` so any remaining malformed stream — inline image or
        // otherwise — is a page-level tool error, never a worker abort.
        // `Content::decode` takes `&[u8]`; wrap the call so the closure is
        // `UnwindSafe` without relying on `AssertUnwindSafe`.
        let decode_result = std::panic::catch_unwind(|| {
            lopdf::content::Content::decode(&content).map(|_| ())
        });
        match decode_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                return Err(TextIndexError::extraction_failed(format!(
                    "Failed to extract PDF text: invalid content stream (page {page_number}): {err}"
                )));
            }
            Err(_) => {
                return Err(TextIndexError::extraction_failed(format!(
                    "Failed to extract PDF text: invalid content stream (page {page_number}): content stream parse failure"
                )));
            }
        }
    }
    Ok(())
}

/// Validate `BI ... ID <data> EI` constructs in a raw content stream.
///
/// For each inline image, the expected data length is computed from the
/// inline dict (`/W`+`/H`+`/BPC`, plus `/CS` component count unless
/// `/IM true`), and `EI` must follow exactly after that many data bytes
/// (modulo the single whitespace lopdf requires on each side). Shapes that
/// cannot yield a length — missing `/CS` without `/IM true`, unknown
/// colorspaces, `/Filter` entries, zero dimensions — are rejected here
/// instead of reaching lopdf's `unwrap()`. Unknown operators or non-inline
/// content passes through untouched; only a structurally-invalid inline
/// image fails validation.
fn validate_inline_images(content: &[u8]) -> Result<(), String> {
    let mut cursor = content;
    while let Some(bi_offset) = find_inline_begin(cursor) {
        cursor = &cursor[bi_offset + 2..];
        let after_bi = skip_content_whitespace(cursor);
        // Parse `key value` pairs until the `ID` operator.
        let mut dict: Vec<(&[u8], &[u8])> = Vec::new();
        let mut rest = after_bi;
        let data_start = loop {
            rest = skip_content_whitespace(rest);
            if rest.len() >= 2 && rest[0] == b'I' && rest[1] == b'D' && is_id_terminator(rest.get(2)) {
                break &rest[2..];
            }
            let (key, after_key) = take_inline_name(rest)
                .ok_or_else(|| "inline image missing ID operator".to_string())?;
            let after_key = skip_content_whitespace(after_key);
            let (value, after_value) = take_inline_value(after_key)
                .ok_or_else(|| "inline image has malformed dict value".to_string())?;
            dict.push((key, value));
            rest = after_value;
        };
        // lopdf requires exactly one whitespace byte after `ID`.
        let data = data_start.strip_prefix(b" ")
            .or_else(|| data_start.strip_prefix(b"\n"))
            .or_else(|| data_start.strip_prefix(b"\r"))
            .or_else(|| data_start.strip_prefix(b"\t"))
            .ok_or_else(|| "inline image ID not followed by whitespace".to_string())?;
        let expected = inline_image_data_len(&dict)?;
        let image_data = data.get(..expected).ok_or_else(|| {
            "inline image data shorter than /W /H /BPC imply".to_string()
        })?;
        // Guard the `EI`-in-data trap from the other direction too: image
        // data containing `EI`-like bytes must not confuse validation, since
        // the length — not a literal scan — determines where data ends.
        let _ = image_data;
        let after_data = &data[expected..];
        // lopdf reads `EI` as `(content_space, tag(b"EI"), content_space)`:
        // whitespace is required before `EI` (the data-length computation
        // consumes the separator) and at least one trailing whitespace byte
        // must follow for the parse to complete.
        let ei = after_data.strip_prefix(b" ")
            .or_else(|| after_data.strip_prefix(b"\n"))
            .or_else(|| after_data.strip_prefix(b"\r"))
            .or_else(|| after_data.strip_prefix(b"\t"))
            .ok_or_else(|| "inline image data not followed by EI".to_string())?;
        if ei.len() < 2 || ei[0] != b'E' || ei[1] != b'I' {
            return Err("inline image data not followed by EI".to_string());
        }
        if ei.len() < 3 || !is_content_whitespace(ei[2]) {
            return Err("inline image EI not followed by whitespace".to_string());
        }
        cursor = &ei[2..];
    }
    Ok(())
}

/// Length of inline-image data implied by the inline dict entries.
fn inline_image_data_len(dict: &[(&[u8], &[u8])]) -> Result<usize, String> {
    let lookup = |abbr: &[u8], full: &[u8]| -> Option<&[u8]> {
        dict.iter()
            .find(|(key, _)| *key == abbr || *key == full)
            .map(|(_, value)| *value)
    };
    let parse_int = |raw: &[u8]
| -> Option<i64> {
        std::str::from_utf8(raw).ok()?.trim().parse::<i64>().ok()
    };
    let width = lookup(b"W", b"Width")
        .and_then(parse_int)
        .ok_or_else(|| "inline image missing /W".to_string())?;
    let height = lookup(b"H", b"Height")
        .and_then(parse_int)
        .ok_or_else(|| "inline image missing /H".to_string())?;
    let bpc = lookup(b"BPC", b"BitsPerComponent")
        .and_then(parse_int)
        .ok_or_else(|| "inline image missing /BPC".to_string())?;
    if width <= 0 || height <= 0 || bpc <= 0 {
        return Err("inline image has non-positive /W /H /BPC".to_string());
    }
    if lookup(b"F", b"Filter").is_some() {
        return Err("filtered inline images are unsupported".to_string());
    }
    let is_mask = lookup(b"IM", b"ImageMask").is_some_and(|raw| raw == b"true");
    let components: usize = if is_mask {
        1
    } else {
        let cs = lookup(b"CS", b"ColorSpace")
            .ok_or_else(|| "inline image missing /CS".to_string())?;
        match cs {
            b"G" | b"DeviceGray" | b"Gray" => 1,
            b"RGB" | b"DeviceRGB" => 3,
            b"CMYK" | b"DeviceCMYK" => 4,
            _ => return Err("inline image has unsupported colorspace".to_string()),
        }
    };
    let width = width as usize;
    let height = height as usize;
    let bpc = bpc as usize;
    let row_bits = width.checked_mul(components.checked_mul(bpc).ok_or_else(|| {
        "inline image dimensions overflow".to_string()
    })?).ok_or_else(|| "inline image dimensions overflow".to_string())?;
    let stride = row_bits.div_ceil(8);
    height.checked_mul(stride).ok_or_else(|| {
        "inline image dimensions overflow".to_string()
    })
}

fn is_content_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn skip_content_whitespace(mut input: &[u8]) -> &[u8] {
    while input.first().is_some_and(|byte| is_content_whitespace(*byte)) {
        input = &input[1..];
    }
    input
}

/// `ID` ends the inline dict only when followed by whitespace; `ID` glued to
/// data (e.g. `ID\x00`) belongs to the data run, matching lopdf which parses
/// `pair(tag(b"ID"), content_space)`.
fn is_id_terminator(next: Option<&u8>) -> bool {
    next.is_some_and(|byte| is_content_whitespace(*byte))
}

/// Find a `BI` operator token (not a prefix of a longer operator such as
/// `BIT`).
fn find_inline_begin(content: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index + 2 <= content.len() {
        if content[index] == b'B' && content[index + 1] == b'I' {
            let prev_ok = index == 0 || is_content_whitespace(content[index - 1]);
            let next = content.get(index + 2);
            let next_ok = next.is_none_or(|byte| is_content_whitespace(*byte));
            if prev_ok && next_ok {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

/// Take a `/Name` (without the leading slash) from an inline dict.
fn take_inline_name(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = input.strip_prefix(b"/")?;
    let end = rest
        .iter()
        .position(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'*' | b'\'' | b'"' | b'_' | b'.' | b'-'))?;
    if end == 0 {
        return None;
    }
    Some((&rest[..end], &rest[end..]))
}

/// Take one inline-dict value: boolean, number, `/Name`, `[array]`, or
/// `(string)`. Returns the raw bytes and the remainder.
fn take_inline_value(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let first = *input.first()?;
    if first == b'/' {
        let (name, rest) = take_inline_name(input)?;
        // Reconstruct the `/Name` span length from the remainder pointers.
        let consumed = input.len() - rest.len();
        let _ = name;
        return Some((&input[1..consumed], rest));
    }
    if first == b'[' {
        let end = input.iter().position(|byte| *byte == b']')?;
        return Some((&input[..end + 1], &input[end + 1..]));
    }
    if first == b'(' {
        // Literal string with nesting/escapes.
        let mut depth = 0usize;
        let mut index = 0usize;
        let mut escaped = false;
        while index < input.len() {
            let byte = input[index];
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'(' {
                depth += 1;
            } else if byte == b')' {
                depth -= 1;
                if depth == 0 {
                    return Some((&input[..index + 1], &input[index + 1..]));
                }
            }
            index += 1;
        }
        return None;
    }
    // Boolean, number, or bare keyword: run to the next whitespace.
    let end = input
        .iter()
        .position(|byte| is_content_whitespace(*byte))
        .unwrap_or(input.len());
    if end == 0 {
        return None;
    }
    Some((&input[..end], &input[end..]))
}

pub fn extract_page_texts(path: &Path, max_file_bytes: u64) -> Result<Vec<String>, TextIndexError> {
    let extracted = extract_pdf_text(path, max_file_bytes)?;

    Ok(extracted.pages.into_iter().map(|page| page.text).collect())
}

#[allow(clippy::too_many_arguments)]
pub fn search_pdf_text(
    path: &Path,
    max_file_bytes: u64,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    max_pages: u32,
    max_matches: u32,
    context_chars: u32,
) -> Result<TextSearchResult, TextIndexError> {
    search_pdf_text_pages(
        path,
        max_file_bytes,
        query,
        case_sensitive,
        whole_word,
        None,
        max_pages,
        max_matches,
        context_chars,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn search_pdf_text_pages(
    path: &Path,
    max_file_bytes: u64,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    requested_pages: Option<&[u32]>,
    max_pages: u32,
    max_matches: u32,
    context_chars: u32,
) -> Result<TextSearchResult, TextIndexError> {
    if query.is_empty() {
        return Err(TextIndexError::invalid_params("query must not be empty."));
    }

    // TS 3.0.14 does not persist extracted document text beside the source.
    // Keep search side-effect free; a future cache needs an explicit private-root contract.
    let extracted = extract_pdf_text(path, max_file_bytes)?;
    let pages = extracted.pages;
    let page_cache = None;
    let num_pages = pages.len().max(1) as u32;
    let searched_pages: Vec<u32> = requested_pages
        .map(|requested| requested.to_vec())
        .unwrap_or_else(|| (1..=num_pages).collect())
        .into_iter()
        .filter(|page| *page <= num_pages)
        .take(max_pages.max(1) as usize)
        .collect();
    let mut matches = Vec::new();
    let mut truncated = false;

    for &page in &searched_pages {
        let page_index = (page - 1) as usize;
        let page_text = pages.get(page_index);
        for (text_item_index, item) in page_text
            .map(|page| page.positioned_items.iter())
            .into_iter()
            .flatten()
            .enumerate()
        {
            let text = &item.text;
            let source_projection = SourceUtf16Projection::new(text);
            let geometry_index = TextGeometryIndex::new(item);
            let remaining = max_matches.saturating_add(1) as usize - matches.len();
            let item_matches = find_matches_in_text_bounded(
                text,
                query,
                case_sensitive,
                whole_word,
                remaining,
                &source_projection,
            )?;

            for item_match in item_matches {
                if matches.len() >= max_matches as usize {
                    truncated = true;
                    break;
                }

                let matched_text = text[item_match.source_start..item_match.source_end].to_string();
                let snippet = build_snippet(
                    text,
                    &source_projection,
                    item_match.start_utf16,
                    item_match.end_utf16,
                    context_chars as usize,
                )?;
                let (bounding_box, bounding_box_level) =
                    geometry_index.match_bounding_box(item_match.start_utf16, item_match.end_utf16);
                matches.push(TextSearchMatch {
                    id: format!("p{page}-match-{}", matches.len() + 1),
                    page,
                    text: matched_text,
                    snippet,
                    match_start: item_match.start_utf16,
                    match_end: item_match.end_utf16,
                    text_item_index: text_item_index as u32,
                    bounding_box,
                    bounding_box_level,
                    route: TEXT_INDEX_ROUTE.into(),
                });
            }

            if truncated {
                break;
            }
        }

        if truncated {
            break;
        }
    }

    Ok(TextSearchResult {
        num_pages,
        searched_pages,
        total_matches: matches.len() as u32,
        matches,
        route: TEXT_INDEX_ROUTE.into(),
        truncated,
        page_cache,
    })
}

struct TextGeometryIndex<'a> {
    item: &'a PositionedTextItem,
    eligible: Vec<usize>,
    #[cfg(test)]
    candidate_visits: std::cell::Cell<usize>,
}

impl<'a> TextGeometryIndex<'a> {
    fn new(item: &'a PositionedTextItem) -> Self {
        // Extraction emits monotonic half-open UTF-16 ranges. Exclude reversed
        // internal geometry fail-closed instead of making every match pay for
        // an unbounded two-dimensional malformed-range scan.
        let mut eligible = item
            .chars
            .iter()
            .enumerate()
            .filter(|(_, character)| {
                !character.is_whitespace
                    && character.item_char_end >= character.item_char_start
                    && character.bounding_box.is_some()
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if eligible
            .windows(2)
            .any(|pair| item.chars[pair[0]].item_char_start > item.chars[pair[1]].item_char_start)
        {
            eligible.sort_unstable_by_key(|index| {
                let character = &item.chars[*index];
                (character.item_char_start, character.item_char_end, *index)
            });
        }
        Self {
            item,
            eligible,
            #[cfg(test)]
            candidate_visits: std::cell::Cell::new(0),
        }
    }

    fn range_candidates(&self, start_utf16: u32, end_utf16: u32) -> &[usize] {
        let first = self
            .eligible
            .partition_point(|index| self.item.chars[*index].item_char_start < start_utf16);
        let count = self.eligible[first..]
            .partition_point(|index| self.item.chars[*index].item_char_start <= end_utf16);
        &self.eligible[first..first + count]
    }

    fn match_bounding_box(
        &self,
        start_utf16: u32,
        end_utf16: u32,
    ) -> (Option<TextBoundingBox>, Option<String>) {
        let char_box = self
            .range_candidates(start_utf16, end_utf16)
            .iter()
            .map(|index| {
                #[cfg(test)]
                self.candidate_visits
                    .set(self.candidate_visits.get().saturating_add(1));
                &self.item.chars[*index]
            })
            .filter(|character| character.item_char_end <= end_utf16)
            .filter_map(|character| character.bounding_box)
            .try_fold(None::<TextBoundingBox>, |current, box_| match current {
                None => Some(Some(box_)),
                Some(current) => current.union(box_).map(Some),
            })
            .flatten();
        if let Some(box_) = char_box {
            (Some(box_), Some("char_estimated".to_string()))
        } else if let Some(box_) = self.item.bounding_box {
            (Some(box_), Some("text_item".to_string()))
        } else {
            (None, None)
        }
    }

    #[cfg(test)]
    fn candidate_visits(&self) -> usize {
        self.candidate_visits.get()
    }
}

#[cfg(test)]
fn match_bounding_box(
    item: &PositionedTextItem,
    start_utf16: u32,
    end_utf16: u32,
) -> (Option<TextBoundingBox>, Option<String>) {
    TextGeometryIndex::new(item).match_bounding_box(start_utf16, end_utf16)
}

fn is_word_char(value: Option<char>) -> bool {
    matches!(value, Some(ch) if ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_whole_word_match(text: &str, start: usize, end: usize) -> bool {
    let before = text.get(..start).and_then(|s| s.chars().last());
    let after = text.get(end..).and_then(|s| s.chars().next());
    !is_word_char(before) && !is_word_char(after)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextMatchRange {
    source_start: usize,
    source_end: usize,
    start_utf16: u32,
    end_utf16: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiteralTextMatch {
    pub(crate) text: String,
    pub(crate) snippet: String,
    pub(crate) start_utf16: u32,
    pub(crate) end_utf16: u32,
}

#[derive(Debug)]
struct NormalizedUtf16Offsets {
    by_byte: Vec<(usize, usize)>,
}

impl NormalizedUtf16Offsets {
    fn new(text: &str) -> Self {
        let mut by_byte = Vec::with_capacity(text.chars().count() + 1);
        let mut utf16_offset = 0usize;
        for (byte_offset, character) in text.char_indices() {
            by_byte.push((byte_offset, utf16_offset));
            utf16_offset += character.len_utf16();
        }
        by_byte.push((text.len(), utf16_offset));
        Self { by_byte }
    }

    fn utf16_at_byte(&self, byte_offset: usize) -> Option<usize> {
        self.by_byte
            .binary_search_by_key(&byte_offset, |(byte, _)| *byte)
            .ok()
            .map(|index| self.by_byte[index].1)
    }
}

#[derive(Debug)]
struct SourceUtf16Projection {
    by_utf16: Vec<(usize, usize)>,
    utf16_len: usize,
}

impl SourceUtf16Projection {
    fn new(text: &str) -> Self {
        let mut by_utf16 = Vec::with_capacity(text.chars().count() + 1);
        let mut utf16_offset = 0usize;
        for (byte_offset, character) in text.char_indices() {
            by_utf16.push((utf16_offset, byte_offset));
            utf16_offset += character.len_utf16();
        }
        by_utf16.push((utf16_offset, text.len()));
        Self {
            by_utf16,
            utf16_len: utf16_offset,
        }
    }

    fn byte_at_utf16_clamped(
        &self,
        utf16_offset: usize,
        split_message: &'static str,
    ) -> Result<usize, TextIndexError> {
        let utf16_offset = utf16_offset.min(self.utf16_len);
        self.by_utf16
            .binary_search_by_key(&utf16_offset, |(units, _)| *units)
            .ok()
            .map(|index| self.by_utf16[index].1)
            .ok_or_else(|| TextIndexError::invalid_request(split_message))
    }
}

fn normalize_for_search(value: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        value.to_string()
    } else {
        value.chars().flat_map(char::to_lowercase).collect()
    }
}

/// Match in the normalized UTF-16 index space used by TS v3.0.14, then apply
/// those offsets directly to the original text for its `slice` semantics.
/// Boundary tables keep projection bounded and fail closed where JavaScript
/// could create a lone surrogate that Rust strings cannot represent.
#[cfg(test)]
fn find_matches_in_text(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Result<Vec<TextMatchRange>, TextIndexError> {
    let source_projection = SourceUtf16Projection::new(text);
    find_matches_in_text_bounded(
        text,
        query,
        case_sensitive,
        whole_word,
        usize::MAX,
        &source_projection,
    )
}

fn find_matches_in_text_bounded(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    max_results: usize,
    source_projection: &SourceUtf16Projection,
) -> Result<Vec<TextMatchRange>, TextIndexError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    let searchable_text = normalize_for_search(text, case_sensitive);
    let searchable_query = normalize_for_search(query, case_sensitive);
    if searchable_query.is_empty() || searchable_text.len() < searchable_query.len() {
        return Ok(matches);
    }
    let normalized_offsets = NormalizedUtf16Offsets::new(&searchable_text);
    let mut search_from = 0usize;
    while search_from + searchable_query.len() <= searchable_text.len() {
        let Some(relative_start) = searchable_text[search_from..].find(&searchable_query) else {
            break;
        };
        let start = search_from + relative_start;
        let end = start + searchable_query.len();
        if !whole_word || is_whole_word_match(&searchable_text, start, end) {
            let start_utf16 = normalized_offsets
                .utf16_at_byte(start)
                .expect("substring search starts at a UTF-8 character boundary");
            let end_utf16 = normalized_offsets
                .utf16_at_byte(end)
                .expect("substring search ends at a UTF-8 character boundary");
            let source_start = source_projection.byte_at_utf16_clamped(
                start_utf16,
                "UTF-16 search projection would split an astral character.",
            )?;
            let source_end = source_projection.byte_at_utf16_clamped(
                end_utf16,
                "UTF-16 search projection would split an astral character.",
            )?;
            matches.push(TextMatchRange {
                source_start,
                source_end,
                start_utf16: start_utf16.try_into().map_err(|_| {
                    TextIndexError::invalid_request("UTF-16 search offset exceeds u32 range.")
                })?,
                end_utf16: end_utf16.try_into().map_err(|_| {
                    TextIndexError::invalid_request("UTF-16 search offset exceeds u32 range.")
                })?,
            });
            if matches.len() >= max_results {
                break;
            }
        }
        search_from = end.max(start + 1);
    }

    Ok(matches)
}

fn build_snippet(
    text: &str,
    source_projection: &SourceUtf16Projection,
    start_utf16: u32,
    end_utf16: u32,
    context_chars: usize,
) -> Result<String, TextIndexError> {
    let start_utf16 = start_utf16 as usize;
    let end_utf16 = end_utf16 as usize;
    let snippet_start_utf16 = start_utf16.saturating_sub(context_chars);
    let snippet_end_utf16 = end_utf16
        .saturating_add(context_chars)
        .min(source_projection.utf16_len);
    let snippet_start = source_projection.byte_at_utf16_clamped(
        snippet_start_utf16,
        "UTF-16 snippet context would split an astral character.",
    )?;
    let snippet_end = source_projection.byte_at_utf16_clamped(
        snippet_end_utf16,
        "UTF-16 snippet context would split an astral character.",
    )?;
    let prefix = if snippet_start > 0 { "..." } else { "" };
    let suffix = if snippet_end < text.len() { "..." } else { "" };
    Ok(format!(
        "{prefix}{}{suffix}",
        &text[snippet_start..snippet_end]
    ))
}

/// Provider-neutral bounded literal matching with the UTF-16 offsets and
/// snippet semantics used by TS v3.0.14. OCR fusion reuses this rather than
/// maintaining a second search implementation.
pub(crate) fn search_literal_text_bounded(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    context_chars: usize,
    max_results: usize,
) -> Result<Vec<LiteralTextMatch>, TextIndexError> {
    let source_projection = SourceUtf16Projection::new(text);
    find_matches_in_text_bounded(
        text,
        query,
        case_sensitive,
        whole_word,
        max_results,
        &source_projection,
    )?
    .into_iter()
    .map(|range| {
        Ok(LiteralTextMatch {
            text: text[range.source_start..range.source_end].to_string(),
            snippet: build_snippet(
                text,
                &source_projection,
                range.start_utf16,
                range.end_utf16,
                context_chars,
            )?,
            start_utf16: range.start_utf16,
            end_utf16: range.end_utf16,
        })
    })
    .collect()
}

#[cfg(test)]
mod tests;
