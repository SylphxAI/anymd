use crate::text_index::TextIndexError;

#[derive(Debug)]
pub(in crate::text_index) struct NormalizedUtf16Offsets {
    pub(in crate::text_index) by_byte: Vec<(usize, usize)>,
}

impl NormalizedUtf16Offsets {
    pub(in crate::text_index) fn new(text: &str) -> Self {
        let mut by_byte = Vec::with_capacity(text.chars().count() + 1);
        let mut utf16_offset = 0usize;
        for (byte_offset, character) in text.char_indices() {
            by_byte.push((byte_offset, utf16_offset));
            utf16_offset += character.len_utf16();
        }
        by_byte.push((text.len(), utf16_offset));
        Self { by_byte }
    }

    pub(in crate::text_index) fn utf16_at_byte(&self, byte_offset: usize) -> Option<usize> {
        self.by_byte
            .binary_search_by_key(&byte_offset, |(byte, _)| *byte)
            .ok()
            .map(|index| self.by_byte[index].1)
    }
}

#[derive(Debug)]
pub(in crate::text_index) struct SourceUtf16Projection {
    pub(in crate::text_index) by_utf16: Vec<(usize, usize)>,
    pub(in crate::text_index) utf16_len: usize,
}

impl SourceUtf16Projection {
    pub(in crate::text_index) fn new(text: &str) -> Self {
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

    pub(in crate::text_index) fn byte_at_utf16_clamped(
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

pub(in crate::text_index) fn normalize_for_search(value: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        value.to_string()
    } else {
        value.chars().flat_map(char::to_lowercase).collect()
    }
}
