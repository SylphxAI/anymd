//! Collect bounded glyph geometry and normalize it into positioned text segments.

use std::collections::BTreeMap;

use pdf_extract::{ColorSpace, MediaBox, OutputDev, OutputError, Path as PdfPath, Transform};

use super::{
    PositionedTextItem, PositionedTextRun, TextBoundingBox, TextCharacterGeometry, TextIndexError,
    MAX_EXTRACTED_TEXT_BYTES, MAX_GEOMETRY_CHARS, MAX_NORMALIZED_TEXT_SEGMENTS,
    MAX_NORMALIZED_TEXT_SEGMENTS_PER_PAGE, MAX_RAW_TEXT_PARTS, MAX_RAW_TEXT_PARTS_PER_PAGE,
    MAX_TOTAL_EXTRACTED_TEXT_BYTES, MAX_TOTAL_GEOMETRY_CHARS, TEXT_SEGMENT_GAP_THRESHOLD,
};

/// A glyph's extent along the text direction and its font size, kept beside
/// each character so word spaces can be inferred from real glyph gaps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GlyphExtent {
    pub(super) x0: f64,
    pub(super) x1: f64,
    pub(super) size: f64,
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
pub(super) struct RawTextPart {
    pub(super) item: PositionedTextItem,
    /// One entry per `item.chars` entry.
    pub(super) glyphs: Vec<Option<GlyphExtent>>,
    pub(super) x: Option<f64>,
    pub(super) y: Option<f64>,
    pub(super) right: Option<f64>,
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
pub(super) struct TextItemOutput {
    pub(super) pages: Vec<Vec<RawTextPart>>,
    pub(super) current_part: Option<RawTextPart>,
    pub(super) current_item_utf16_len: u32,
    pub(super) current_item_geometry_valid: bool,
    pub(super) text_bytes: usize,
    pub(super) geometry_chars: usize,
    pub(super) total_text_bytes: usize,
    pub(super) total_geometry_chars: usize,
    pub(super) raw_part_count: usize,
    pub(super) page_raw_part_count: usize,
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

    pub(super) fn admit_part(&mut self) -> Result<(), OutputError> {
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

pub(super) fn normalize_page_text_parts(
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
