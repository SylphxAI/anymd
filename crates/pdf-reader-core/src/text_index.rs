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

    fn extraction_failed(message: impl Into<String>) -> Self {
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

#[derive(Debug)]
struct RawTextPart {
    item: PositionedTextItem,
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

fn merge_raw_text_segment(parts: Vec<RawTextPart>) -> Result<PositionedTextItem, TextIndexError> {
    let mut text = String::new();
    let mut chars = Vec::new();
    let mut runs = Vec::with_capacity(parts.len());
    let mut bounding_box = None;
    let mut offset = 0u32;

    for part in parts {
        let run_start = offset;
        let run_len = part
            .item
            .text
            .encode_utf16()
            .count()
            .try_into()
            .map_err(|_| TextIndexError::extraction_failed("selectable text offset overflow"))?;
        offset = offset
            .checked_add(run_len)
            .ok_or_else(|| TextIndexError::extraction_failed("selectable text offset overflow"))?;
        text.push_str(&part.item.text);
        for mut character in part.item.chars {
            character.item_char_start = run_start
                .checked_add(character.item_char_start)
                .ok_or_else(|| {
                    TextIndexError::extraction_failed("selectable text character offset overflow")
                })?;
            character.item_char_end =
                run_start
                    .checked_add(character.item_char_end)
                    .ok_or_else(|| {
                        TextIndexError::extraction_failed(
                            "selectable text character offset overflow",
                        )
                    })?;
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
            text: part.item.text,
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
                Ok(ExtractedPageText {
                    text: items.concat(),
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
mod tests {
    use super::*;

    fn raw_part(text: &str, x: f64, y: f64, width: f64) -> RawTextPart {
        let bounding_box = TextBoundingBox {
            left: x,
            bottom: y,
            right: x + width,
            top: y + 10.0,
        };
        let text_len = text.encode_utf16().count() as u32;
        let mut offset = 0u32;
        let chars = text
            .chars()
            .map(|character| {
                let value = character.to_string();
                let start = offset;
                offset += value.encode_utf16().count() as u32;
                TextCharacterGeometry {
                    text: value,
                    item_char_start: start,
                    item_char_end: offset,
                    is_whitespace: character.is_whitespace(),
                    bounding_box: bounding_box.estimated_utf16_range(text_len, start, offset),
                }
            })
            .collect();
        RawTextPart {
            item: PositionedTextItem {
                text: text.to_string(),
                bounding_box: Some(bounding_box),
                chars,
                runs: Vec::new(),
            },
            x: Some(x),
            y: Some(y),
            right: Some(x + width),
        }
    }

    #[test]
    fn ltr_normalizer_matches_js_round_gap_and_stable_x_semantics() {
        let mut request_segments = 0;
        let items = normalize_page_text_parts(
            vec![
                raw_part("B", 60.0, 500.10, 10.0),
                raw_part("D", 0.0, 500.50, 10.0),
                raw_part("C", 118.01, 500.49, 10.0),
                raw_part("A", 2.0, 500.49, 10.0),
            ],
            &mut request_segments,
        )
        .expect("normalize LTR parts");

        assert_eq!(
            items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>(),
            vec!["D", "AB", "C"]
        );
        assert_eq!(items[1].runs.len(), 2);
        assert_eq!(items[1].bounding_box.unwrap().left, 2.0);
        assert_eq!(items[1].bounding_box.unwrap().right, 70.0);
        assert_eq!(request_segments, 3);
    }

    #[test]
    fn ltr_normalizer_uses_running_max_right_and_rebases_utf16_runs_and_chars() {
        let mut request_segments = 0;
        let items = normalize_page_text_parts(
            vec![
                raw_part("A😀", 0.0, 100.0, 100.0),
                raw_part("B", 20.0, 100.0, 5.0),
                raw_part("C", 148.0, 100.0, 5.0),
            ],
            &mut request_segments,
        )
        .expect("normalize overlapping parts");

        assert_eq!(items.len(), 1, "48-point gap from running max-right joins");
        assert_eq!(items[0].text, "A😀BC");
        assert_eq!(items[0].runs[0].item_char_end, 3);
        assert_eq!(items[0].runs[1].item_char_start, 3);
        assert_eq!(items[0].runs[2].item_char_start, 4);
        assert_eq!(items[0].chars[1].item_char_end, 3);
        assert_eq!(items[0].chars[2].item_char_start, 3);
        assert_eq!(items[0].chars[3].item_char_start, 4);
    }

    #[test]
    fn raw_part_admission_accepts_exact_caps_and_rejects_cap_plus_one() {
        let mut output = TextItemOutput::default();
        for _ in 0..MAX_RAW_TEXT_PARTS_PER_PAGE {
            output.admit_part().expect("exact page cap");
        }
        assert!(output.admit_part().is_err(), "page cap + 1 must fail");

        let mut output = TextItemOutput::default();
        for index in 0..MAX_RAW_TEXT_PARTS {
            if index % MAX_RAW_TEXT_PARTS_PER_PAGE == 0 {
                output.page_raw_part_count = 0;
            }
            output.admit_part().expect("exact request cap");
        }
        output.page_raw_part_count = 0;
        assert!(output.admit_part().is_err(), "request cap + 1 must fail");

        let mut output = TextItemOutput {
            raw_part_count: MAX_RAW_TEXT_PARTS,
            ..TextItemOutput::default()
        };
        assert!(output.begin_word().is_err());
        assert!(
            output.current_part.is_none(),
            "cap rejection must precede raw-part allocation"
        );
    }

    #[test]
    fn normalized_segment_admission_is_exact_and_never_returns_partial_output() {
        let exact = (0..MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE)
            .map(|index| raw_part("x", index as f64 * 60.0, 100.0, 1.0))
            .collect();
        let mut request_segments = 0;
        assert_eq!(
            normalize_page_text_parts(exact, &mut request_segments)
                .expect("exact normalized page cap")
                .len(),
            MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE
        );

        let over = (0..=MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE)
            .map(|index| raw_part("x", index as f64 * 60.0, 100.0, 1.0))
            .collect();
        let mut request_segments = 0;
        assert!(normalize_page_text_parts(over, &mut request_segments).is_err());

        let mut request_segments = MAX_NORMALIZED_TEXT_SEGMENTS - 1;
        normalize_page_text_parts(vec![raw_part("x", 0.0, 0.0, 1.0)], &mut request_segments)
            .expect("exact normalized request cap");
        assert_eq!(request_segments, MAX_NORMALIZED_TEXT_SEGMENTS);
        assert!(normalize_page_text_parts(
            vec![raw_part("x", 0.0, 0.0, 1.0)],
            &mut request_segments
        )
        .is_err());
    }

    fn two_blank_page_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Resources << >> /Contents 5 0 R >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Resources << >> /Contents 6 0 R >>".to_string(),
            "<< /Length 0 >>\nstream\nendstream".to_string(),
            "<< /Length 0 >>\nstream\nendstream".to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn positioned_text_pdf(text: &str) -> Vec<u8> {
        let content = format!("BT /F1 12 Tf 1 0 0 1 72 700 Tm ({text}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn multi_page_text_pdf(page_texts: &[String]) -> Vec<u8> {
        let page_count = page_texts.len();
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} >>",
                (3..3 + page_count)
                    .map(|index| format!("{index} 0 R"))
                    .collect::<Vec<_>>()
                    .join(" "),
                page_count
            ),
        ];
        let mut contents_object = 3 + page_count;
        let font_object = contents_object + page_count;
        for _text in page_texts {
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font_object} 0 R >> >> /Contents {contents_object} 0 R >>"
            ));
            contents_object += 1;
        }
        for text in page_texts {
            let content =
                format!("BT /F1 12 Tf 1 0 0 1 72 700 Tm ({text}) Tj ET");
            objects.push(format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ));
        }
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
        let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn per_page_budget_reset_accepts_dense_multi_page_documents() {
        // Two pages whose cumulative text exceeds the old request-wide
        // geometry budget, while each page stays inside the per-page cap.
        let page_texts = vec!["x".repeat(130_000), "y".repeat(130_000)];
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("dense-two-pages.pdf");
        std::fs::write(&path, multi_page_text_pdf(&page_texts)).expect("write PDF");
        let pages = extract_page_texts(&path, 4_000_000).expect("dense pages must extract");
        assert_eq!(pages, page_texts);
    }

    #[test]
    fn single_page_over_per_page_budget_still_fails_closed() {
        let page_texts = vec!["x".repeat(MAX_GEOMETRY_CHARS + 1)];
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("over-dense-page.pdf");
        std::fs::write(&path, multi_page_text_pdf(&page_texts)).expect("write PDF");
        let error = extract_page_texts(&path, 4_000_000).expect_err("per-page cap must hold");
        assert!(
            error.message.contains("bounded extraction budget"),
            "unexpected error: {:?}",
            error
        );
    }

    #[test]
    fn document_backstop_still_fails_closed() {
        let mut output = TextItemOutput {
            total_geometry_chars: MAX_TOTAL_GEOMETRY_CHARS,
            total_text_bytes: MAX_TOTAL_EXTRACTED_TEXT_BYTES,
            pages: vec![Vec::new()],
            ..TextItemOutput::default()
        };
        output.begin_word().expect("begin word");
        let error = output
            .output_character(&Transform::identity(), 1.0, 0.0, 12.0, "x")
            .expect_err("document backstop must fail closed");
        assert!(
            error
                .to_string()
                .contains("bounded document extraction budget"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn page_counters_reset_and_document_counters_accumulate() {
        let mut output = TextItemOutput {
            text_bytes: MAX_EXTRACTED_TEXT_BYTES,
            geometry_chars: MAX_GEOMETRY_CHARS,
            total_text_bytes: 3,
            total_geometry_chars: 3,
            pages: vec![Vec::new()],
            ..TextItemOutput::default()
        };
        output.begin_word().expect("begin word");
        assert!(
            output
                .output_character(&Transform::identity(), 1.0, 0.0, 12.0, "x")
                .is_err(),
            "exhausted per-page budget must fail before begin_page resets it"
        );
        output
            .begin_page(2, &MediaBox { llx: 0.0, lly: 0.0, urx: 612.0, ury: 792.0 }, None)
            .expect("begin page");
        output.begin_word().expect("begin word");
        output
            .output_character(&Transform::identity(), 1.0, 0.0, 12.0, "ab")
            .expect("fresh per-page budget after begin_page");
        // Note: the rejected character above still incremented the cumulative
        // totals (admission happens before the cap check), so they start at 4.
        assert_eq!(output.text_bytes, 2);
        assert_eq!(output.geometry_chars, 1);
        assert_eq!(output.total_text_bytes, 6);
        assert_eq!(output.total_geometry_chars, 5);
    }

    #[test]
    fn preserves_real_pdf_page_boundaries_including_blank_pages() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("two-pages.pdf");
        std::fs::write(&path, two_blank_page_pdf()).expect("write PDF");
        let pages = extract_page_texts(&path, 1_000_000).expect("extract pages");
        assert_eq!(pages, vec![String::new(), String::new()]);
    }

    #[test]
    fn captures_bounded_selectable_text_geometry_and_search_match_boxes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("positioned.pdf");
        std::fs::write(&path, positioned_text_pdf("Alpha beta")).expect("write PDF");

        let extracted = extract_pdf_text(&path, 1_000_000).expect("extract geometry");
        let item = &extracted.pages[0].positioned_items[0];
        assert_eq!(item.text, "Alpha beta");
        assert_eq!(item.chars.len(), 10);
        assert_eq!(item.chars[0].item_char_start, 0);
        assert_eq!(item.chars[0].item_char_end, 1);
        assert!(item.bounding_box.is_some());
        assert!(item
            .chars
            .iter()
            .all(|character| character.bounding_box.is_some()));

        let result = search_pdf_text(&path, 1_000_000, "beta", true, false, 1, 5, 10)
            .expect("search geometry");
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].match_start, 6);
        assert_eq!(result.matches[0].match_end, 10);
        assert_eq!(
            result.matches[0].bounding_box_level.as_deref(),
            Some("char_estimated")
        );
        let match_box = result.matches[0].bounding_box.expect("match box");
        let item_box = item.bounding_box.expect("item box");
        assert!(match_box.left > item_box.left);
        assert_eq!(match_box.right, item_box.right);
    }

    #[test]
    fn captures_geometry_for_behavior_fixture() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential/v3014-behavior-v1.pdf");
        let extracted = extract_pdf_text(&path, 1_000_000).expect("extract fixture");
        let needle_items = extracted
            .pages
            .iter()
            .flat_map(|page| &page.positioned_items)
            .filter(|item| item.text.to_lowercase().contains("needle"))
            .collect::<Vec<_>>();
        assert!(
            needle_items.iter().all(|item| item.bounding_box.is_some()),
            "needle items: {needle_items:?}"
        );
    }

    #[test]
    fn geometry_is_finite_utf16_aware_and_bounded() {
        let item_box = TextBoundingBox {
            left: 10.0,
            bottom: 20.0,
            right: 50.0,
            top: 30.0,
        };
        // A😀B has four JavaScript UTF-16 code units; the astral character owns two.
        let astral = item_box
            .estimated_utf16_range(4, 1, 3)
            .expect("astral range box");
        assert_eq!(astral.left, 20.0);
        assert_eq!(astral.right, 40.0);

        let nonfinite = Transform::row_major(1.0, 0.0, 0.0, 1.0, f64::NAN, 20.0);
        assert!(TextBoundingBox::from_character(&nonfinite, 1.0, 12.0).is_none());

        let mut output = TextItemOutput {
            text_bytes: MAX_EXTRACTED_TEXT_BYTES,
            ..TextItemOutput::default()
        };
        output.begin_word().expect("begin word");
        let error = output
            .output_character(&Transform::identity(), 1.0, 0.0, 12.0, "x")
            .expect_err("budget must fail closed");
        assert!(error.to_string().contains("bounded extraction budget"));
    }

    #[test]
    fn source_coordinates_canonicalize_f32_expansion_at_four_decimal_places() {
        assert_eq!(canonical_coordinate(699.499_023_438), Some(699.499));
        assert_eq!(canonical_coordinate(123.000_999_451), Some(123.001));
        assert_eq!(canonical_coordinate(701.000_000_047), Some(701.0));
        assert_eq!(canonical_coordinate(-0.000_001), Some(0.0));
        assert!(!canonical_coordinate(-0.000_001)
            .expect("canonical zero")
            .is_sign_negative());
        assert_eq!(canonical_coordinate(f64::NAN), None);
        assert_eq!(canonical_coordinate(f64::INFINITY), None);
        assert_eq!(canonical_coordinate(f64::MAX), None);

        let transform = Transform::row_major(1.0, 0.0, 0.0, 1.0, 123.000_999_451, 699.499_023_438);
        let box_ = TextBoundingBox::from_character(&transform, 1.0, 1.500_976_609)
            .expect("canonical character box");
        assert_eq!(box_.left, 123.001);
        assert_eq!(box_.bottom, 699.499);
        assert_eq!(box_.right, 124.502);
        assert_eq!(box_.top, 701.0);
    }

    #[test]
    fn opposite_extreme_finite_coordinates_fail_item_geometry_closed() {
        let left = TextBoundingBox {
            left: -1.0e308,
            bottom: 0.0,
            right: -9.0e307,
            top: 12.0,
        };
        let right = TextBoundingBox {
            left: 9.0e307,
            bottom: 0.0,
            right: 1.0e308,
            top: 12.0,
        };
        assert!(left.union(right).is_none());
        assert!(TextBoundingBox {
            left: -1.0e308,
            bottom: 0.0,
            right: 1.0e308,
            top: 12.0,
        }
        .estimated_utf16_range(2, 0, 1)
        .is_none());

        let mut output = TextItemOutput {
            pages: vec![Vec::new()],
            ..TextItemOutput::default()
        };
        output.begin_word().expect("begin word");
        let left_transform = Transform::row_major(1.0, 0.0, 0.0, 1.0, -1.0e308, 10.0);
        let right_transform = Transform::row_major(1.0, 0.0, 0.0, 1.0, 1.0e308, 10.0);
        output
            .output_character(&left_transform, 1.0, 0.0, 12.0, "a")
            .expect("left character");
        output
            .output_character(&right_transform, 1.0, 0.0, 12.0, "b")
            .expect("right character");
        output.end_line().expect("end line");
        let item = &output.pages[0][0];
        assert_eq!(item.item.bounding_box, None);
        assert!(item
            .item
            .chars
            .iter()
            .all(|character| character.bounding_box.is_none()));
    }

    #[test]
    fn individual_invalid_character_geometry_is_sticky_in_both_orders() {
        let valid = Transform::row_major(1.0, 0.0, 0.0, 1.0, 72.0, 700.0);
        let invalid = Transform::row_major(1.0, 0.0, 0.0, 1.0, f64::NAN, 700.0);
        for transforms in [[&invalid, &valid], [&valid, &invalid]] {
            let mut output = TextItemOutput {
                pages: vec![Vec::new()],
                ..TextItemOutput::default()
            };
            output.begin_word().expect("begin word");
            for (index, transform) in transforms.into_iter().enumerate() {
                output
                    .output_character(
                        transform,
                        1.0,
                        0.0,
                        12.0,
                        if index == 0 { "a" } else { "b" },
                    )
                    .expect("character callback remains recoverable");
            }
            output.end_line().expect("end line");
            let item = &output.pages[0][0];
            assert_eq!(item.item.bounding_box, None);
            assert!(item
                .item
                .chars
                .iter()
                .all(|character| character.bounding_box.is_none()));
        }
    }

    #[test]
    fn whitespace_only_match_falls_back_to_text_item_box() {
        let item_box = TextBoundingBox {
            left: 10.0,
            bottom: 20.0,
            right: 50.0,
            top: 30.0,
        };
        let item = PositionedTextItem {
            text: " ".into(),
            bounding_box: Some(item_box),
            chars: vec![TextCharacterGeometry {
                text: " ".into(),
                item_char_start: 0,
                item_char_end: 1,
                is_whitespace: true,
                bounding_box: Some(item_box),
            }],
            runs: Vec::new(),
        };
        let (box_, level) = match_bounding_box(&item, 0, 1);
        assert_eq!(box_, Some(item_box));
        assert_eq!(level.as_deref(), Some("text_item"));
    }

    #[test]
    fn geometry_index_bounds_exact_cap_match_queries() {
        let box_ = TextBoundingBox {
            left: 0.0,
            bottom: 0.0,
            right: 1.0,
            top: 1.0,
        };
        let mut chars = Vec::with_capacity(MAX_GEOMETRY_CHARS);
        for offset in 0..MAX_GEOMETRY_CHARS as u32 {
            chars.push(TextCharacterGeometry {
                text: String::new(),
                item_char_start: offset,
                item_char_end: offset + 1,
                is_whitespace: false,
                bounding_box: Some(box_),
            });
        }
        let item = PositionedTextItem {
            text: String::new(),
            bounding_box: None,
            chars,
            runs: Vec::new(),
        };
        let index = TextGeometryIndex::new(&item);
        for offset in 0..500u32 {
            assert_eq!(
                index.match_bounding_box(offset, offset + 1),
                (Some(box_), Some("char_estimated".to_string()))
            );
        }
        assert_eq!(
            index.candidate_visits(),
            1_000,
            "500 production matcher calls must range-query the index instead of rescanning all {MAX_GEOMETRY_CHARS} chars"
        );
    }

    fn reference_match_bounding_box(
        item: &PositionedTextItem,
        start_utf16: u32,
        end_utf16: u32,
    ) -> (Option<TextBoundingBox>, Option<String>) {
        let char_box = item
            .chars
            .iter()
            .filter(|character| {
                !character.is_whitespace
                    && character.item_char_end >= character.item_char_start
                    && character.item_char_start >= start_utf16
                    && character.item_char_end <= end_utf16
            })
            .filter_map(|character| character.bounding_box)
            .try_fold(None::<TextBoundingBox>, |current, box_| match current {
                None => Some(Some(box_)),
                Some(current) => current.union(box_).map(Some),
            })
            .flatten();
        if let Some(box_) = char_box {
            (Some(box_), Some("char_estimated".to_string()))
        } else if let Some(box_) = item.bounding_box {
            (Some(box_), Some("text_item".to_string()))
        } else {
            (None, None)
        }
    }

    #[test]
    fn geometry_index_matches_reference_for_irregular_internal_ranges() {
        let item_box = TextBoundingBox {
            left: -10.0,
            bottom: -10.0,
            right: 10.0,
            top: 10.0,
        };
        let boxes = [
            TextBoundingBox {
                left: 2.0,
                bottom: 0.0,
                right: 3.0,
                top: 1.0,
            },
            TextBoundingBox {
                left: 0.0,
                bottom: 0.0,
                right: 1.0,
                top: 1.0,
            },
            TextBoundingBox {
                left: 1.0,
                bottom: 0.0,
                right: 2.0,
                top: 1.0,
            },
        ];
        let item = PositionedTextItem {
            text: "abc".to_string(),
            bounding_box: Some(item_box),
            chars: vec![
                TextCharacterGeometry {
                    text: "c".to_string(),
                    item_char_start: 2,
                    item_char_end: 3,
                    is_whitespace: false,
                    bounding_box: Some(boxes[0]),
                },
                TextCharacterGeometry {
                    text: "".to_string(),
                    item_char_start: 1,
                    item_char_end: 1,
                    is_whitespace: false,
                    bounding_box: Some(boxes[1]),
                },
                TextCharacterGeometry {
                    text: "malformed".to_string(),
                    item_char_start: 2,
                    item_char_end: 1,
                    is_whitespace: false,
                    bounding_box: Some(boxes[2]),
                },
                TextCharacterGeometry {
                    text: "b".to_string(),
                    item_char_start: 1,
                    item_char_end: 2,
                    is_whitespace: false,
                    bounding_box: Some(boxes[1]),
                },
                TextCharacterGeometry {
                    text: "ignored".to_string(),
                    item_char_start: 0,
                    item_char_end: 3,
                    is_whitespace: true,
                    bounding_box: Some(boxes[0]),
                },
                TextCharacterGeometry {
                    text: "unboxed".to_string(),
                    item_char_start: 0,
                    item_char_end: 1,
                    is_whitespace: false,
                    bounding_box: None,
                },
            ],
            runs: Vec::new(),
        };
        let index = TextGeometryIndex::new(&item);
        for (start, end) in [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 3), (3, 3)] {
            assert_eq!(
                index.match_bounding_box(start, end),
                reference_match_bounding_box(&item, start, end),
                "range {start}..{end}"
            );
        }
    }

    #[test]
    fn finds_literal_matches_with_snippets() {
        let text = "Sample PDF file with sample terms inside.";
        let matches = find_matches_in_text(text, "sample", false, false).unwrap();
        assert_eq!(matches.len(), 2);
        let projection = SourceUtf16Projection::new(text);
        let snippet = build_snippet(
            text,
            &projection,
            matches[0].start_utf16,
            matches[0].end_utf16,
            6,
        )
        .unwrap();
        assert!(snippet.contains("Sample PDF"));
    }

    #[test]
    fn respects_whole_word_boundaries() {
        let text = "risk risky risk";
        let matches = find_matches_in_text(text, "risk", false, true).unwrap();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn does_not_panic_on_multibyte_ligature_when_searching_ascii() {
        // ﬁ lowercases to "fi" (2 chars); naive byte-index search used to panic in snippets.
        let text = "preﬁx and more text around here for context";
        let matches = find_matches_in_text(text, "a", false, false).unwrap();
        assert!(!matches.is_empty());
        let projection = SourceUtf16Projection::new(text);
        for item_match in matches {
            let _ = build_snippet(
                text,
                &projection,
                item_match.start_utf16,
                item_match.end_utf16,
                40,
            )
            .unwrap();
            assert!(text.is_char_boundary(item_match.source_start));
            assert!(text.is_char_boundary(item_match.source_end));
        }
    }

    #[test]
    fn reports_offsets_in_javascript_utf16_code_units() {
        let text = "😀Café";
        let matches = find_matches_in_text(text, "Café", true, false).unwrap();
        assert_eq!(matches[0].start_utf16, 2);
        assert_eq!(matches[0].end_utf16, 6);
    }

    #[test]
    fn projects_length_changing_lowercase_indices_like_v3_0_14() {
        let text = "A İX Z ASCII ";

        let capital_i_dot = find_matches_in_text(text, "İ", false, false).unwrap()[0];
        assert_eq!((capital_i_dot.start_utf16, capital_i_dot.end_utf16), (2, 4));
        assert_eq!(
            &text[capital_i_dot.source_start..capital_i_dot.source_end],
            "İX"
        );

        let combining_dot = find_matches_in_text(text, "\u{307}", false, false).unwrap()[0];
        assert_eq!((combining_dot.start_utf16, combining_dot.end_utf16), (3, 4));
        assert_eq!(
            &text[combining_dot.source_start..combining_dot.source_end],
            "X"
        );

        let ascii = find_matches_in_text(text, "ASCII", false, true).unwrap()[0];
        assert_eq!((ascii.start_utf16, ascii.end_utf16), (8, 13));
        assert_eq!(&text[ascii.source_start..ascii.source_end], "SCII ");
    }

    #[test]
    fn lowercase_index_projection_fails_closed_at_split_surrogate() {
        let error = find_matches_in_text("İ😀", "😀", false, false).unwrap_err();
        assert_eq!(error.code, TextIndexErrorCode::InvalidRequest);
        assert!(error.message.contains("split an astral character"));
    }

    #[test]
    fn lowercase_index_projection_preserves_clamping_nonoverlap_and_normalized_words() {
        let clamped_text = "Aİ";
        let clamped = find_matches_in_text(clamped_text, "\u{307}", false, false).unwrap()[0];
        assert_eq!((clamped.start_utf16, clamped.end_utf16), (2, 3));
        assert_eq!(clamped.source_start, clamped_text.len());
        assert_eq!(clamped.source_end, clamped_text.len());
        let projection = SourceUtf16Projection::new(clamped_text);
        assert_eq!(
            build_snippet(
                clamped_text,
                &projection,
                clamped.start_utf16,
                clamped.end_utf16,
                0,
            )
            .unwrap(),
            "..."
        );

        let nonoverlapping = find_matches_in_text("aaaa", "aa", false, false).unwrap();
        assert_eq!(
            nonoverlapping
                .iter()
                .map(|range| (range.start_utf16, range.end_utf16))
                .collect::<Vec<_>>(),
            vec![(0, 2), (2, 4)]
        );

        // The combining dot before `A` exists only in normalized text. TS
        // therefore admits this whole-word match even though direct projection
        // to the original string would inspect a different neighbour.
        let normalized_word = find_matches_in_text("İA", "A", false, true).unwrap()[0];
        assert_eq!(
            (normalized_word.start_utf16, normalized_word.end_utf16),
            (2, 3)
        );
        assert_eq!(normalized_word.source_start, "İA".len());
        assert_eq!(normalized_word.source_end, "İA".len());

        // Conversely, the normalized `A` immediately before this space is a
        // word character, so TS rejects it even though the same projected
        // original boundary has a space as its preceding character.
        assert!(find_matches_in_text("İA ", " ", false, true)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn snippet_context_counts_utf16_units_like_v3_0_14() {
        let text = "Café résumé";
        let item_match = find_matches_in_text(text, "résumé", true, false).unwrap()[0];
        let projection = SourceUtf16Projection::new(text);
        assert_eq!(
            build_snippet(
                text,
                &projection,
                item_match.start_utf16,
                item_match.end_utf16,
                5,
            )
            .unwrap(),
            "Café résumé"
        );
        assert_eq!(
            build_snippet(
                text,
                &projection,
                item_match.start_utf16,
                item_match.end_utf16,
                0,
            )
            .unwrap(),
            "...résumé"
        );

        // Rust strings cannot contain lone UTF-16 surrogates. Fail closed when
        // the exact TS context boundary would bisect an astral scalar.
        let astral = "A😀résuméZ";
        let item_match = find_matches_in_text(astral, "résumé", true, false).unwrap()[0];
        let projection = SourceUtf16Projection::new(astral);
        let error = build_snippet(
            astral,
            &projection,
            item_match.start_utf16,
            item_match.end_utf16,
            1,
        )
        .unwrap_err();
        assert_eq!(error.code, TextIndexErrorCode::InvalidRequest);
        assert!(error.message.contains("split an astral character"));
        assert_eq!(
            build_snippet(
                astral,
                &projection,
                item_match.start_utf16,
                item_match.end_utf16,
                2,
            )
            .unwrap(),
            "...😀résuméZ"
        );

        let trailing_astral = "résumé😀A";
        let item_match = find_matches_in_text(trailing_astral, "résumé", true, false).unwrap()[0];
        let projection = SourceUtf16Projection::new(trailing_astral);
        let error = build_snippet(
            trailing_astral,
            &projection,
            item_match.start_utf16,
            item_match.end_utf16,
            1,
        )
        .unwrap_err();
        assert_eq!(error.code, TextIndexErrorCode::InvalidRequest);
        assert_eq!(
            build_snippet(
                trailing_astral,
                &projection,
                item_match.start_utf16,
                item_match.end_utf16,
                2,
            )
            .unwrap(),
            "résumé😀..."
        );
    }

    #[test]
    fn search_does_not_persist_extracted_text_beside_source() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/sample.pdf");
        if !fixture.is_file() {
            return;
        }

        let temp = tempfile::tempdir().expect("tempdir");
        let pdf_path = temp.path().join("sample.pdf");
        std::fs::copy(&fixture, &pdf_path).expect("copy fixture");

        let first = search_pdf_text(
            pdf_path.as_path(),
            1024 * 1024,
            "Lorem",
            false,
            false,
            10,
            10,
            32,
        )
        .expect("first search");
        assert_eq!(first.page_cache, None);
        assert!(first.total_matches > 0);

        let second = search_pdf_text(
            pdf_path.as_path(),
            1024 * 1024,
            "Lorem",
            false,
            false,
            10,
            10,
            32,
        )
        .expect("second search");
        assert_eq!(second.page_cache, None);
        assert_eq!(second.total_matches, first.total_matches);
        assert!(!temp.path().join(".pdf-reader-mcp").exists());
    }

    #[test]
    fn bulk_normalize_case_and_empty_query() {
        assert_eq!(
            find_matches_in_text("AbC", "abc", false, false)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            find_matches_in_text("AbC", "AbC", true, false)
                .unwrap()
                .len(),
            1
        );
        assert!(find_matches_in_text("AbC", "abc", true, false)
            .unwrap()
            .is_empty());
        assert!(find_matches_in_text("hello", "", false, false)
            .unwrap()
            .is_empty());
        assert!(find_matches_in_text("hello", "z", false, false)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn bulk_whole_word_and_snippet_ellipsis() {
        let text = "alpha beta alphabet";
        let whole = find_matches_in_text(text, "alpha", false, true).unwrap();
        assert_eq!(whole.len(), 1, "{whole:?}");
        let loose = find_matches_in_text(text, "alpha", false, false).unwrap();
        assert!(loose.len() >= 2, "{loose:?}");
        assert!(is_word_char(Some('a')));
        assert!(is_word_char(Some('_')));
        assert!(!is_word_char(Some(' ')));
        assert!(!is_word_char(None));
        let snippet_text = "0123456789abcdefghij";
        let projection = SourceUtf16Projection::new(snippet_text);
        let snip = build_snippet(snippet_text, &projection, 8, 12, 2).unwrap();
        assert!(snip.contains("..."), "{snip}");
        let short = "short";
        let short_projection = SourceUtf16Projection::new(short);
        let snip2 = build_snippet(short, &short_projection, 0, 5, 10).unwrap();
        assert_eq!(snip2, "short");
    }

    #[test]
    fn extraction_contains_cff_custom_encoding_charset_panic() {
        // Regression for SylphxAI/pdf-reader-mcp#660. pdf-extract unwraps
        // cff-parser while loading a Type1C font. A malformed Custom encoding
        // that is longer than its charset used to panic at encoding.rs and
        // escape as a server/task abort instead of a structured MCP error.
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential/v3014-bug660-cff-custom-encoding-short-charset-v1.pdf");
        let error = extract_pdf_text(&fixture, 256 * 1024 * 1024)
            .expect_err("CFF Custom encoding panic must be contained");
        assert_eq!(error.code, TextIndexErrorCode::ExtractionFailed);
        assert!(error.message.contains("malformed font encoding"));
    }

    fn inline_image_pdf(content: &[u8]) -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << >> >>"
                .to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content.len(),
                String::from_utf8_lossy(content)
            ),
        ];
        let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn write_inline_pdf(content: &[u8]) -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("inline.pdf"), inline_image_pdf(content))
            .expect("write PDF");
        temp
    }

    #[test]
    fn extraction_survives_wellformed_inline_images() {
        // Regression for SylphxAI/pdf-reader-mcp#675: the issue's reproducer
        // (`BI /W 1 /H 1 /IM true /BPC 1 ID <0x00> EI`) and its variant table
        // (grey colorspace, full key names, 8x8 data, `q..Q` wrapping, 0xFF
        // data byte, `EI`-like bytes in data) must extract without panicking.
        let variants: &[&[u8]] = &[
            b"BI /W 1 /H 1 /IM true /BPC 1 ID \x00 EI\n",
            b"BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI\n",
            b"BI /W 8 /H 8 /IM true /BPC 1 ID \x00\x00\x00\x00\x00\x00\x00\x00 EI\n",
            b"BI /Width 1 /Height 1 /ImageMask true /BitsPerComponent 1 ID \x00 EI\n",
            b"q BI /W 1 /H 1 /IM true /BPC 1 ID \x00 EI Q\n",
            b"BI /W 1 /H 1 /IM true /BPC 1 ID \xFF EI\n",
            b"BI /W 16 /H 1 /IM true /BPC 1 ID EI EI\n",
            b"BI /W 1 /H 1 /CS /DeviceGray /BPC 8 ID \x00 EI\n",
        ];
        for (index, content) in variants.iter().enumerate() {
            let temp = write_inline_pdf(content);
            let extracted = extract_pdf_text(&temp.path().join("inline.pdf"), 256 * 1024 * 1024)
                .unwrap_or_else(|err| panic!("variant {index} must extract: {}", err.message));
            assert_eq!(extracted.pages.len(), 1, "variant {index}");
        }
    }

    #[test]
    fn extraction_reports_malformed_inline_image_as_page_error() {
        // Regression for SylphxAI/pdf-reader-mcp#675 request 1: a malformed
        // content stream must surface as a page-level tool error naming the
        // page — never a panicked worker. A missing `/CS` (without `/IM`)
        // panics inside lopdf 0.42's inline-image parser (`unwrap()` of
        // `DictKey("ColorSpace")`); it must arrive here as
        // `invalid content stream (page 1)`.
        let malformed: &[u8] = b"BI /W 1 /H 1 /BPC 1 ID \x00 EI\n";
        let temp = write_inline_pdf(malformed);
        let error = extract_pdf_text(&temp.path().join("inline.pdf"), 256 * 1024 * 1024)
            .expect_err("malformed inline image must be a structured error");
        assert_eq!(error.code, TextIndexErrorCode::ExtractionFailed);
        assert!(
            error.message.contains("invalid content stream (page 1)"),
            "unexpected message: {}",
            error.message
        );
    }

    #[test]
    fn extraction_rejects_truncated_inline_image_data() {
        // An 8x8 1-bit mask needs 8 data bytes; 2 bytes must fail as a
        // page-level error rather than a literal-`EI` scan or a panic.
        let truncated: &[u8] = b"BI /W 8 /H 8 /IM true /BPC 1 ID \x00\x00 EI\n";
        let temp = write_inline_pdf(truncated);
        let error = extract_pdf_text(&temp.path().join("inline.pdf"), 256 * 1024 * 1024)
            .expect_err("truncated inline image must be a structured error");
        assert_eq!(error.code, TextIndexErrorCode::ExtractionFailed);
        assert!(
            error.message.contains("invalid content stream (page 1)"),
            "unexpected message: {}",
            error.message
        );
    }

    #[test]
    fn extraction_survives_cid_cmap_with_odd_length_bfrange_destination() {
        // Regression for SylphxAI/pdf-reader-mcp#608. pdfTeX files ship ToUnicode
        // CMaps with 1-byte beginbfrange destinations (e.g. <C5> <D6> <C5>), which
        // made the upstream adobe-cmap-parser panic with "bad length of hexstring".
        // pdf-extract unwraps that Result, so the panic used to abort the whole
        // pdf-reader-mcp process. This fixture reproduces the exact crashing
        // construct and must extract without panicking.
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/differential/v3014-bug608-cid-bfrange-odd-v1.pdf");
        let extracted = extract_pdf_text(&fixture, 256 * 1024 * 1024)
            .expect("malformed CMap must not abort extraction");
        assert_eq!(extracted.pages.len(), 1);
        assert_eq!(extracted.info.format_version, "1.4");

        // The search path is the same one exposed over MCP; it must complete.
        let search = search_pdf_text(&fixture, 256 * 1024 * 1024, "a", false, false, 100, 50, 120)
            .expect("malformed CMap must not abort search");
        assert_eq!(search.num_pages, 1);
        assert_eq!(search.truncated, false);
    }
}
