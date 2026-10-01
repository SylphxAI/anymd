//! # OAR OCR VL
//!
//! Vision-Language models for document understanding, integrating with oar-ocr-core.
//!
//! ## Modules
//!
//! - `paddleocr_vl` - PaddleOCR-VL, 1.5, and 1.6 for OCR, table, formula,
//!   chart, spotting, and seal recognition
//! - `hunyuanocr` - HunyuanOCR 1.5 / 1.0 OCR expert VLM
//! - `hpd_parsing` - HPD-Parsing hierarchical parallel document parser
//! - `glmocr` - GLM-OCR OCR expert VLM
//! - `mineru` - MinerU2.5 and MinerU2.5-Pro document parsing VLMs (Qwen2-VL
//!   backbone)
//! - `mineru_diffusion` - MinerU-Diffusion-V1 block-diffusion document OCR
//!   (Qwen2-VL vision + SDAR decoder)
//! - `monkeyocrv2` - MonkeyOCRv2-S/B-Parsing full-page and region parsing
//! - `ovisocr2` - OvisOCR2 end-to-end page-to-Markdown parser (Qwen3.5)
//! - `doc_parser` - Unified document parsing with pluggable recognition backends
//! - `pp_doclayout` - Native PP-DocLayoutV2/V3 layout detection and reading
//!   order
//! - `layout` - Backend-agnostic [`LayoutSource`] trait feeding `doc_parser`
//! - `utils` - Device parsing, candle helpers, markdown, OTSL conversion
//! - `attention` - Unified attention shared by all models
//!
//! [`LayoutSource`]: layout::LayoutSource
//!
//! ## Candle only
//!
//! Everything here runs on Candle, including layout detection via
//! [`PpDocLayout`], so `ort` never enters the dependency tree. To source layout
//! from somewhere else, implement [`LayoutSource`].
//!
//! GPU acceleration is gated behind the `cuda` feature. Parse device strings
//! with [`utils::parse_device`]:
//!
//! ```no_run
//! use oar_ocr_vl::utils::parse_device;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let device = parse_device("cuda:0")?;
//! # let _ = device;
//! # Ok(())
//! # }
//! ```

// Core model modules
pub mod crop;
pub mod doc_parser;
pub mod error;
pub mod geometry;
pub mod glmocr;
pub mod hpd_parsing;
pub mod hunyuanocr;
pub mod layout;
pub mod mineru;
pub mod mineru_diffusion;
pub mod monkeyocrv2;
pub mod ovisocr2;
pub mod paddleocr_vl;
pub mod pp_doclayout;
pub mod structure;
pub mod utils;

// Shared attention implementation
pub mod attention;

// Small CUDA primitives shared by multiple VLM backends. Model-specific
// kernels remain in their respective modules, while stable sampling and other
// reusable decode operations live here.
#[cfg(feature = "cuda")]
pub(crate) mod cuda_kernels;

// Shared lifetime and capacity plumbing for batch-1 decoder CUDA graphs.
pub(crate) mod decoder_graph;

// `TrimmableKvCache` backs the KV cache used by every model's attention
// forward path.
pub(crate) mod kv_trim;

// Re-exports for convenience
pub use paddleocr_vl::{
    PaddleOcrVl, PaddleOcrVlConfig, PaddleOcrVlImageProcessorConfig, PaddleOcrVlTask,
};

pub use glmocr::GlmOcr;
pub use hpd_parsing::{HpdGenerationConfig, HpdOutput, HpdParsing, HpdRuntimeStats};
pub use hunyuanocr::{DFlashConfig, DFlashTargetConfig, HunyuanOcr, HunyuanOcrVersion};
pub use mineru::MinerU;
pub use mineru_diffusion::{DiffusionGenerationConfig, MinerUDiffusion};
pub use monkeyocrv2::{MonkeyOcrV2, MonkeyOcrV2Task};
pub use ovisocr2::OvisOcr2;

pub use doc_parser::{DocParser, DocParserConfig, RecognitionBackend, RecognitionTask};
pub use error::{BatchResult, Error, ProcessingStage, Result};
pub use geometry::{BoundingBox, Point};
pub use layout::{LayoutDetectionElement, LayoutDetections, LayoutSource, StaticLayout};
pub use pp_doclayout::{PpDocLayout, PpDocLayoutVersion};
pub use structure::{LayoutElement, LayoutElementType, StructureResult, TableResult, TableType};
