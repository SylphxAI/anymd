//! Collect text and glyph geometry from PDF callbacks within extraction budgets.

use pdf_extract::{ColorSpace, MediaBox, OutputDev, OutputError, Path as PdfPath, Transform};

use super::super::{
    TextBoundingBox, TextCharacterGeometry, MAX_EXTRACTED_TEXT_BYTES, MAX_GEOMETRY_CHARS,
    MAX_RAW_TEXT_PARTS, MAX_RAW_TEXT_PARTS_PER_PAGE, MAX_TOTAL_EXTRACTED_TEXT_BYTES,
    MAX_TOTAL_GEOMETRY_CHARS,
};
use super::{GlyphExtent, RawTextPart};

#[derive(Default)]
pub(in crate::text_index) struct TextItemOutput {
    pub(in crate::text_index) pages: Vec<Vec<RawTextPart>>,
    pub(in crate::text_index) current_part: Option<RawTextPart>,
    pub(in crate::text_index) current_item_utf16_len: u32,
    pub(in crate::text_index) current_item_geometry_valid: bool,
    pub(in crate::text_index) text_bytes: usize,
    pub(in crate::text_index) geometry_chars: usize,
    pub(in crate::text_index) total_text_bytes: usize,
    pub(in crate::text_index) total_geometry_chars: usize,
    pub(in crate::text_index) raw_part_count: usize,
    pub(in crate::text_index) page_raw_part_count: usize,
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

    pub(in crate::text_index) fn admit_part(&mut self) -> Result<(), OutputError> {
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
