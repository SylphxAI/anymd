//! MonkeyOCRv2-S/B-Parsing document VLM.
//!
//! The checkpoints combine MonkeyOCRv2 ViT-S or ViT-B encoders with a
//! Qwen3-0.6B causal decoder. This module provides native Candle inference
//! for the official layout, end-to-end, text, formula, and OTSL-table prompts.

mod config;
mod model;
mod processing;
mod vision;

pub use config::{MonkeyOcrV2Config, MonkeyOcrV2ImageProcessorConfig, MonkeyOcrV2VisionConfig};
pub use model::{DEFAULT_MAX_NEW_TOKENS, LAYOUT_MIN_PIXELS, MonkeyOcrV2, MonkeyOcrV2Task};
