use std::time::Instant;

use super::super::request_limits::DEFAULT_MAX_OUTPUT_CHARS;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReadOcrOptions {
    pub(crate) engine: Option<crate::ocr_vlm::OcrEngine>,
    pub(crate) deadline: Option<Instant>,
    pub(crate) exact_geometry: bool,
    pub(crate) scale: f64,
    pub(crate) max_pages: usize,
    pub(crate) max_pixels_per_page: u64,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) max_output_chars: usize,
}

impl Default for ReadOcrOptions {
    fn default() -> Self {
        Self {
            engine: None,
            deadline: None,
            exact_geometry: false,
            scale: 2.0,
            max_pages: 5,
            max_pixels_per_page: 16_000_000,
            timeout_ms: None,
            max_output_chars: DEFAULT_MAX_OUTPUT_CHARS,
        }
    }
}
