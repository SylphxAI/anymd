//! Native Candle implementation of PP-DocLayoutV2 and PP-DocLayoutV3.
//!
//! Both checkpoints share an RT-DETR detector (HGNetV2-L backbone, hybrid
//! encoder, deformable-attention decoder) and differ in how they predict
//! reading order: V2 runs a separate LayoutLM-style pointer network over the
//! surviving boxes, V3 an order head inside the decoder.
//!
//! Running these on Candle is what lets `oar-ocr-vl` stay ONNX-free.
//! [`PpDocLayout`] implements
//! [`LayoutSource`](crate::layout::LayoutSource), so it drops straight into
//! [`DocParser`](crate::DocParser):
//!
//! ```no_run
//! use oar_ocr_vl::pp_doclayout::PpDocLayout;
//! use oar_ocr_vl::utils::parse_device;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let layout = PpDocLayout::from_dir(
//!     "PaddlePaddle/PP-DocLayoutV2_safetensors",
//!     parse_device("cpu")?,
//! )?;
//! # let _ = layout;
//! # Ok(())
//! # }
//! ```
//!
//! Weights are the `model.safetensors` checkpoints published as
//! `PaddlePaddle/PP-DocLayoutV2_safetensors` and
//! `PaddlePaddle/PP-DocLayoutV3_safetensors`.

mod config;
mod decoder;
mod encoder;
mod err;
mod hgnetv2;
mod layers;
mod mask_head;
mod model;
mod order;
mod processing;
mod reading_order;

pub use config::{PpDocLayoutConfig, PpDocLayoutVersion, ReadingOrderConfig};
pub use model::{Detection, PpDocLayout};
