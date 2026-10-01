//! PaddleOCR-VL (Vision-Language) model.
//!
//! Native Rust inference for the 0.9B PaddleOCR-VL, PaddleOCR-VL-1.5, and
//! PaddleOCR-VL-1.6 checkpoints. All three use [`PaddleOcrVl::from_dir`].
//! Supported tasks include:
//! - OCR (text recognition)
//! - Table recognition (outputs HTML)
//! - Formula recognition (outputs LaTeX)
//! - Chart recognition
//! - Text spotting (1.5 and 1.6)
//! - Seal recognition (1.5 and 1.6)

mod config;
mod ernie;
mod model;
mod processing;
mod quantized_linear;
mod projector;
mod vision;

pub use config::{
    PaddleOcrVlConfig, PaddleOcrVlImageProcessorConfig, PaddleOcrVlRopeScaling,
    PaddleOcrVlVisionConfig,
};
pub use model::{PaddleOcrVl, PaddleOcrVlTask};
pub use processing::{
    PaddleOcrVlImageInputs, postprocess_table_output, preprocess_images, smart_resize,
    strip_math_wrappers,
};
