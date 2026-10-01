//! OvisOCR2 document-to-Markdown model support.

mod config;
mod gated_delta;
mod model;
pub(crate) mod processing;
pub(crate) mod text;
pub(crate) mod vision;

pub use config::{
    OVIS_OCR2_MAX_PIXELS, OVIS_OCR2_MIN_PIXELS, OvisOcr2Config, OvisOcr2ImageProcessorConfig,
    OvisOcr2ImageProcessorSize, OvisOcr2RopeParameters, OvisOcr2TextConfig, OvisOcr2VisionConfig,
};
pub use model::{
    DEFAULT_MAX_NEW_TOKENS, DEFAULT_PROMPT, OvisOcr2, clean_truncated_repeats,
    filter_visual_image_tags,
};
