//! HunyuanOCR 1.5 / 1.0 (HunYuanVL) Vision-Language model.
//!
//! This module provides native Rust inference for the `tencent/HunyuanOCR`
//! model (1.5 at the repository root and archived 1.0 under `v1.0/`) using
//! Candle.

mod config;
mod dflash;
#[cfg(feature = "cuda")]
pub(crate) mod dynamic_kv;
mod llm;
mod model;
mod processing;
mod vision;

pub use config::{
    HunyuanOcrConfig, HunyuanOcrImageProcessorConfig, HunyuanOcrRopeScaling, HunyuanOcrVersion,
    HunyuanOcrVisionConfig,
};
pub use dflash::{DFlashConfig, DFlashTargetConfig};
pub use model::HunyuanOcr;
