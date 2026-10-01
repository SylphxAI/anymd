//! MinerU2.5 Vision-Language model implementation (Qwen2-VL backbone).
//!
//! [`MinerU::from_dir`] supports both `MinerU2.5-2509-1.2B` and
//! `MinerU2.5-Pro-2605-1.2B`. For full-page documents, use the model-native
//! two-step layout-detection and crop-recognition pipeline demonstrated by the
//! `mineru` example.

mod config;
mod model;
pub(crate) mod processing;
mod text;
pub(crate) mod vision;

pub use config::{
    MinerUConfig, MinerUImageProcessorConfig, MinerURopeScaling, MinerUTextConfig,
    MinerUVisionConfig,
};
pub use model::MinerU;
